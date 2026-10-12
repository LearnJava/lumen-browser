# BUG-1626 — Lumen с Chrome-UA принимает -moz-appearance и другие -moz-префиксы

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** css-parser/layout (`crates/engine/css-parser/src/lib.rs:68`, `values/length.rs:268`)

## Симптом и место в коде

Найдено попутно: при Chrome-идентичности Lumen принимает Firefox-префиксы, которых в Chrome нет: `-moz-appearance` в SUPPORTED_PROPERTIES (css-parser/src/lib.rs:68), отсюда CSS.supports и @supports дают true; применяется arm-ами appearance (style/apply/css_wide.rs:434, apply/paint.rs:533, cascade.rs:1128); `-moz-available` как Length::Stretch (style/values/length.rs:268); префиксы `-moz-` у значений unicode-bidi (style/values/typography.rs:164). `@supports (-moz-appearance:none)` — распространённая обёртка Firefox-специфичных CSS-хаков, `CSS.supports('-moz-appearance','none')` — частый детектор Firefox. Поведение Chrome по памяти (Chrome не разбирает -moz-), живой пробой в Edge не подтверждено. Решение «следовать Chrome» согласуется с правилом (а)/(б)/(в) из areas/08 CMP-08.

## Где смотреть

- crates/engine/css-parser/src/lib.rs:68
- crates/engine/layout/src/style/apply/css_wide.rs:434
- crates/engine/layout/src/style/apply/paint.rs:533
- crates/engine/layout/src/style/cascade.rs:1128
- crates/engine/layout/src/style/values/length.rs:268
- crates/engine/layout/src/style/values/typography.rs:164

## Объём и приёмка

При Chrome-UA Lumen принимает Firefox-префиксы, которых нет в Chrome: `-moz-appearance` в SUPPORTED_PROPERTIES (css-parser/src/lib.rs:68) и arm-ах (style/apply/css_wide.rs:434, apply/paint.rs:533, cascade.rs:1128), `-moz-available` (values/length.rs:268), `-moz-` у значений unicode-bidi (values/typography.rs:164). `CSS.supports('-moz-appearance','none')` — частый детектор Firefox, `@supports (-moz-appearance:none)` — обёртка Firefox-хаков: Lumen их применяет, Chrome — нет (по коду, пробой не подтверждено). Приёмка: CSS.supports('-moz-appearance','none') = false, как в Edge. Источник: report.md §3 п.2.

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
