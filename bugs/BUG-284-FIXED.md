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

## Результат

| | до | после |
|---|---|---|
| `build_box` (cascade) | 1274 мс | 613 мс |
| `precompute_counters` | 1054 мс | 465 мс |
| relayout итого | ~2.4 с | ~1.15 с |
| relayout-штормов подряд | 6× (~13.6с фриз) | 0 |

Побочный эффект: при более быстром каскаде первая пачка асинхронных
DOM-мутаций (картинки, рекламные скрипты) укладывается в один relayout —
штормы из нескольких relayout подряд перестали воспроизводиться в этом
сценарии (не гасились искусственно, а перестали возникать).

## Тесты

`cargo test -p lumen-layout --lib` — 3189/3189 зелёных (индексация @media/
@layer/@supports не сломала каскад). `cargo clippy -p lumen-layout
--all-targets -- -D warnings` — чисто.

`graphic_tests/run.py --continue-on-fail` (dev-release): 39 FAIL + 38 DEBTOR —
**«Дельта vs предыдущий прогон: Изменений нет»** — идентично эталонному
прогону на main до фикса (предсуществующий фон, не регрессия от этого
изменения).

## Остаток

`build_box`/`precompute_counters` всё ещё ~0.6мс/узел — заметно быстрее, но
не «мгновенно». Возможные дальнейшие цели (не в этой задаче): убрать
дублирующий полный cascade-проход в `precompute_counters` (он уже вызывает
тот же `compute_style`, что и `build_box`, — де-факто два полных прохода по
дереву на один relayout); индексация `@scope`/`@container`, если найдутся
реальные сайты с существенным числом правил там.
