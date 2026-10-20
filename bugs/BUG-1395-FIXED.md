# BUG-1395 — `contain: style` не действует: счётчики и кавычки внутри поддерева выходят наружу

**Статус:** FIXED 2026-10-20 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout (`crates/engine/layout/src/counters.rs::precompute_counters`, разбор кавычек; флаг `ContainFlags::STYLE` читается только в `selector_query.rs:2080`)

## Симптом

`--dump-layout`, `body{margin:0;font:20px/1 Ahem}`:

| страница | получено | ожидается |
|---|---|---|
| `#o{counter-reset:n}` `#o i{counter-increment:n}` `#t::before{content:counter(n)}`; `<div id=o><div style="contain:style"><i></i><i></i></div><b id=t></b></div>` | `2` | `0` (инкременты внутри поддерева `contain:style` не выходят за него; CSS Containment L2, style containment) |
| то же без `contain:style` | `2` | `2` |
| `div{quotes:"A" "Z" "1" "9"} div::before,span::before{content:open-quote} div::after{content:close-quote} span{contain:style}`; `<div><span></span></div>` | `A19` | `A1Z` (глубина кавычек возвращается на границе `span`) |

Две страницы с `contain:style` и без дают одинаковый результат: флаг в раскладке не используется. `grep -rn ContainFlags::STYLE crates` — только разбор (`style/apply/layout.rs:913`) и `getComputedStyle` (`selector_query.rs:2080`).

## Как найдено

WPT-RUN-14 срез 17: `css-contain/counter-scoping-001…004.html`, `quote-scoping-001…003.html`, `quote-scoping-invalidation-001…004.html`, `contain-style-counters-002…005.html`, `contain-style-dynamic-001.html`, `contain-style-ol-ordinal-pseudo{,-reversed}.html` — 21 id (20 reftest, 1 testharness). `counter-scoping-001…004` дополнительно упираются в [BUG-1368](BUG-1368-OPEN.md) (`counter-increment` на самом `::after` не применяется) — проверять после него.

## Что делать

В `precompute_counters` (`counters.rs`) при входе в элемент с `STYLE` сохранять значения счётчиков и восстанавливать при выходе (инкременты и `counter-set` не видны снаружи; сам элемент создаёт новые экземпляры счётчиков). То же для стека кавычек.

## Как проверить

`css/css-contain/counter-scoping-002.html`, `quote-scoping-001.html`, `contain-style-counters-003.html`.

## Исправление (2026-10-20, P6)

`CounterCtx` (`counters.rs`) знает границу `contain: style`: `enter_style_boundary` запоминает глубину стека каждого счётчика (после собственного `counter-reset` элемента) и `quote_depth`; `counter-increment`/`counter-set` ниже границы через `ensure_instance` заводят новый экземпляр вместо внешнего, `pop_fresh` снимает экземпляры при выходе из родителя; на выходе из границы `quote_depth` возвращается. Тесты: `contain_style_*` в `counters.rs`.

Не проверено прогоном WPT (на момент слияния): `counter-scoping-002` (`1 1.2` — `counter-set` на `::after` внутри границы), `contain-style-counters-005` (см. [BUG-1658](BUG-1658-OPEN.md)), `contain-style-dynamic-001`, `contain-style-ol-ordinal-pseudo*`, `quote-scoping-invalidation-*`.
