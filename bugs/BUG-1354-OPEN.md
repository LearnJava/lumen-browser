# BUG-1354 — Встроенный `<svg>` внутри блока раскладывается как блок, а не как строчный replaced: нет строки и зазора под базовой линией

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** layout (`crates/engine/layout/src/box_tree/svg.rs`, `inline_build.rs` — встроенный `<svg>` в потоке)

## Симптом

`--dump-layout`: `<div style="width:300px"><svg width="200" height="50"></svg></div>` → `Block h=50`, внутри `SvgRoot`; те же 200×50 у `<img>` и у `inline-block` дают `Block h=54` (`InlineBlockRow`, зазор под базовой линией 4 px). `inline-replaced-width-002.xht`: синий (`svg`) и оранжевый (`div`) прямоугольники у Lumen вплотную, в эталоне между ними белая полоса.

## Как найдено

WPT-RUN-14 срез 12: 13 id `normal-flow/{block-replaced-height-006,inline-block-replaced-height-006,inline-replaced-height-006,inline-replaced-height-009,inline-replaced-width-002,-003,-008,-009,-014,…}` — во всех `<svg:svg>`/`<object>` в потоке рядом с `div`. Причины, кроме строчного размещения, не исключены (часть эталонов — `<object>`).

## Что делать

Класть `<svg>` и `<object>` в строчный поток как inline replaced через `InlineBlockRow` с базовой линией по нижнему краю (CSS 2.1 §10.3.2, §10.6.2).

## Как проверить

`css/CSS2/normal-flow/inline-replaced-width-002.xht`.

## Срез 13 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` tables/positioning/floats/floats-clear/abspos…)

15 id в `floats-clear/float-replaced-height-006.xht`, `float-replaced-width-007.xht`, `-008.xht` и `positioning/absolute-replaced-*` — `<svg>`/`<object>` как replaced-элемент (float/abspos). По правилу отнесения (`<svg|<object` в тесте, thick), пробой не разделено с BUG-1337/1356.
