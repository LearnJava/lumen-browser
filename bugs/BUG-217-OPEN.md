# BUG-217

**Статус:** OPEN (DEBTOR)
**Компонент:** css-parser
**Тест:** TEST-120 (diff 3.26%, KNOWN_DEBTOR)

## Описание

Media Queries L5 §5.5/§5.6 `prefers-contrast` / `prefers-reduced-data` — **реализованы
и спек-корректны** (ревизия 2026-06-23). Парсинг и матчинг живут в
`crates/engine/css-parser/src/parser.rs`:
`MediaFeature::PrefersContrast` / `MediaFeature::PrefersReducedData`
(`parse_media_feature`, матчинг в `MediaFeature::matches`), значения по умолчанию
`no-preference` / `no-preference`, есть юнит-тесты (`media_query_prefers_contrast_*`,
`media_query_prefers_reduced_data_*`).

## Почему остаётся DEBTOR (не дефект движка)

`python graphic_tests/run.py --only 120` → 3.26%. Разбор diff-картинки:

- swatch **`.a`** `(prefers-contrast: no-preference)` → зелёный **в обоих** движках (Edge
  поддерживает `prefers-contrast`). Совпадает.
- swatch **`.b`** `(prefers-reduced-data: no-preference)` → Lumen матчит → **зелёный**
  (как явно требует комментарий тест-страницы: «On a correct engine the two "match"
  swatches are green»). **Edge не поддерживает `prefers-reduced-data`** (фича не
  отгружена в Chromium/Edge) → query не матчится → `.b` остаётся **красным**.

Весь diff 3.26% = единственный swatch `.b`. Lumen корректнее reference-браузера; чтобы
совпасть с Edge, пришлось бы отключить рабочую media-feature — запрещено (rule 4 /
rule 4a). Тот же класс, что:
- BUG-126 / TEST-77 (`inset-area` — Edge игнорит),
- BUG-199 / TEST-71 (`@starting-style` — тайминг захвата Edge),
- BUG-237 / TEST-122 (`line-height-step` — удалён из Chromium).

## Воспроизведение

`python graphic_tests/run.py --only 120` → FAIL 3.26% (KNOWN_DEBTOR baseline).

## Что закрыло бы (нереалистично)

Только воспроизведение лимита Edge (перестать матчить `prefers-reduced-data`), что
ухудшит соответствие спецификации. Поэтому пункт остаётся DEBTOR с baseline 3.26% в
`KNOWN_DEBTORS` (`run.py`).
