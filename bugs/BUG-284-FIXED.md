# BUG-284 — CSS cascade: `@layer`/`@media`/`@supports` rules brute-force scanned per node

**Статус:** FIXED 2026-07-15 (P3)
**Компонент:** layout (`crates/engine/layout/src/style.rs`, `crates/engine/layout/src/rule_index.rs`)
**Найден:** 2026-07-15, ручное тестирование релизной сборки на https://ria.ru

## Симптом

Браузер практически непригоден для использования на реальных новостных сайтах
(https://ria.ru) — постоянные многосекундные фризы, «слайдшоу» вместо
интерактивности. `LUMEN_FRAME_LOG=1` показал: сразу после загрузки страницы
(ещё до какого-либо скролла) — 6 полных синхронных relayout подряд, каждый
по 2.1–2.5с, суммарно ~13.6с UI-поток полностью не отвечал:

```
[bench] first non-empty frame: 7368ms since process start
[engine] relayout 2186.12ms dl=1079 styled=1141
[engine] relayout 2148.86ms dl=1079 styled=1141
[engine] relayout 2274.80ms dl=1079 styled=1141
[engine] relayout 2341.42ms dl=1079 styled=1141
[engine] relayout 2491.30ms dl=1079 styled=1141
[engine] relayout 2173.66ms dl=1079 styled=1141
```

`dl`/`styled` не менялись между вызовами — движок пересчитывал один и тот же
документ (1141 styled-узел) заново, впустую.

## Расследование

Точечная инструментация (`LUMEN_LAYOUT_TRACE`, временная, удалена после
диагностики) разбила один relayout на фазы:

```
[layout-trace]   precompute_counters 1054.56ms
[layout-trace] build_box (cascade) 1274.75ms
[layout-trace] lay_out (geometry) 26.86ms
[layout-trace] post-layout passes 68.72ms
```

Обе крупные фазы вызывают `compute_style` (и `precompute_counters`
дополнительно — `compute_pseudo_element_style` для quote-глубины ::before/
::after) для каждого из 1141 styled-узлов.

## Корень

`RuleIndex` (`rule_index.rs`) индексирует по subject-ключу (id/class/type)
только **верхнеуровневые** `sheet.rules`. Документация модуля прямо
называла это ограничение «Phase 1 scope» с допущением: «rules in
`sheet.layers`/`media_rules`/`supports_rules` … typically a small fraction of
total rules» — для ria.ru (и вообще любого сайта с адаптивной вёрсткой на
`@media`-брейкпоинтах) это допущение неверно: из 3093 правил страницы
значительная часть лежит внутри `@media`.

`compute_style` для каждого узла делал `for rule in &media.rules { for
complex in &rule.selectors { matches_complex(...) } }` — полный перебор всех
правил каждого `@media`/`@layer`/`@supports`-блока, без какой-либо
пре-фильтрации. `compute_pseudo_element_style` (вызывается для **каждого**
элемента дважды — before/after) не был проиндексирован вовсе, даже для
верхнеуровневых `sheet.rules`.

## Фикс

1. `RuleIndex::build` → `RuleIndex::build_from_rules(&[Rule])`, обобщён на
   произвольный срез правил (не только `Stylesheet.rules`).
2. Новый `CascadeIndex` (thread-local, тот же кэш-ключ `sheet_ptr +
   sheet.rules.len()`, что и раньше): индекс верхнего уровня + по одному
   индексу на каждый `@layer`/`@media`/`@supports` блок, построенные один
   раз за layout pass.
3. Циклы в `compute_style` по `sheet.layers`/`media_rules`/`supports_rules`
   переписаны на `candidates()`-поиск вместо перебора; глобальная нумерация
   правил (для порядка каскада) сохранена через исходный `rule_idx` внутри
   блока — порядок обхода кандидатов не важен, важна только позиция в блоке.
4. `compute_pseudo_element_style` получил тот же кэш `CascadeIndex` и
   индексированные циклы (subject-ключ агностичен к `::before`/`::after`,
   добавленному к subject-компаунду — тот же индекс валиден).
5. `@scope`/`@container` **не тронуты** (остаются brute-force) — типично
   меньше правил, ниже риск/выгода не оправдывает расширение скоупа фикса.

## Дополнение (тот же день): переиспользование `ComputedStyle` между проходами

`precompute_counters` и `build_box` — два ПОЛНЫХ отдельных document-order
прохода по дереву, и оба вызывают `compute_style` для каждого узла с
идентичными аргументами (тот же `doc`/`sheet`/`viewport`/`dark_mode`, та же
цепочка `inherited` — оба обхода в одном порядке через `flat.children_of`).
`precompute_counters` всегда выполняется первым и передаёт `CounterMap` в
`build_box` — второй проход буквально пересчитывал то, что первый уже знал.

Фикс: `CounterMap` получил кэш `styles: HashMap<NodeId, ComputedStyle>`,
заполняемый в `precompute_counters::walk` (`map.styles.insert(id,
style.clone())`), и метод `style_for(id)`. `build_box` теперь читает оттуда
(`counters.style_for(id).cloned().unwrap_or_else(|| compute_style(...))` —
fallback на случай узла, для которого кэша нет, например корневой
`NodeData::Document`, который `walk` не обсчитывает).

Проверены все 3 точки входа (`layout_measured_hyp`, `layout`,
`layout_streaming_incremental`) — везде `precompute_counters` вызывается
непосредственно перед `build_box` с теми же аргументами, так что кэш валиден
для каждого текущего вызывающего.

Результат на ria.ru: `build_box` 613мс → 524мс (~15%, меньше, чем ожидалось —
значит доля именно `compute_style` внутри `build_box` уже не доминирует после
основного фикса выше; остальное — построение box-дерева, resolve картинок и
т.д.). `cargo test -p lumen-layout` 3189/3189; точечные graphic-тесты на
counters/quotes/list-markers (32, 97, 117) дали ИДЕНТИЧНЫЕ проценты diff, что
и самый первый прогон на main до всех правок — визуальных регрессий нет.

## Результат

| | до | после фикса 1 (индексация) | после фикса 2 (кэш стилей) |
|---|---|---|---|
| `build_box` (cascade) | 1274 мс | 613 мс | 524 мс |
| `precompute_counters` | 1054 мс | 465 мс | ~465–589 мс (без изменений, ожидаемо — это единственный проход, который теперь реально считает каскад) |
| relayout итого | ~2.4 с | ~1.15 с | ~1.0–1.1 с |
| relayout-штормов подряд | 6× (~13.6с фриз) | 0 | 0 |

Побочный эффект фикса 1: при более быстром каскаде первая пачка асинхронных
DOM-мутаций (картинки, рекламные скрипты) укладывается в один relayout —
штормы из нескольких relayout подряд перестали воспроизводиться в этом
сценарии (не гасились искусственно, а перестали возникать).

## Тесты

`cargo test -p lumen-layout --lib` — 3189/3189 зелёных (обе правки: индексация
@media/@layer/@supports + кэш стилей между precompute_counters/build_box).
`cargo clippy -p lumen-layout --all-targets -- -D warnings` — чисто.

`graphic_tests/run.py --continue-on-fail` (dev-release, фикс 1): 39 FAIL + 38
DEBTOR — **«Дельта vs предыдущий прогон: Изменений нет»** — идентично
эталонному прогону на main до фикса (предсуществующий фон, не регрессия).

`graphic_tests/run.py --only 32/97/117` (фикс 2, counters/quotes/list-markers
— самая чувствительная к кэшу область): проценты diff **идентичны** самому
первому прогону на main до всех правок этой задачи.

## Остаток

`build_box`/`precompute_counters` всё ещё ~0.4–0.5мс/узел — заметно быстрее
(~2.2× от исходных ~2.4с), но не «мгновенно». `precompute_counters` остаётся
единственным полным document-order проходом, вызывающим настоящий каскад
(`compute_style` + `compute_pseudo_element_style` для before/after каждого
элемента) — дальнейший выигрыш потребовал бы либо ускорения самого
`compute_style`/`matches_complex` (per-node overhead помимо кандидатного
поиска: аллокации `Vec`/`BTreeSet`, сборка `node_classes`), либо
архитектурного слияния counter-прохода и build_box в один (рискованно —
`content: counter(...)` должен видеть пост-reset/increment значение СВОЕГО
же узла, что и является причиной текущего two-pass дизайна). Также:
индексация `@scope`/`@container`, если найдутся реальные сайты с
существенным числом правил там.
