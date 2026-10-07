# BUG-1365 — Отрицательный `margin-top`/`margin-bottom` блока с блочными детьми теряется при схлопывании (CSS 2.1 §8.3.1)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/bfc.rs` — `collapsed_top_margin`, схлопывание поля блока с полем первого/последнего ребёнка)

## Симптом

`--dump-layout`, `body{margin:0}`, первый блок `height:100px`:

| разметка после него | получено | ожидается |
|---|---|---|
| `<div style="margin-top:-60px"><div height:20 blue></div></div>` (обёртка с блочным ребёнком) | синий `y=100` | `y=40` |
| `<div style="margin-top:-60px; height:20; blue">` (лист, без детей) | `y=40` (верно) | `y=40` |
| `<div><div style="margin-top:-60px; height:20; blue"></div></div>` | `y=100` | `y=40` |
| первый блок `margin-bottom:30px`, затем обёртка `margin-top:-60px` с блочным ребёнком | `y=130` | `y=70` (30 − 60 = −30) |
| `<div style="margin-bottom:-60px"><div blue height:20></div></div>` + красный `height:10` следом | красный `y=120` | `y=60` |
| обёртка `margin-top:-60px; padding-top:1px` с блочным ребёнком (поля не схлопываются) | `y=41` (верно) | `y=41` |
| обёртка `margin-top:-60px; overflow:hidden` (BFC) | `y=40` (верно) | `y=40` |

`--dump-layout` показывает `m=(-60,0,0,0)` у обёртки — значение разобрано, но смещения не даёт: отрицательное поле пропадает ровно там, где поле родителя схлопывается с полем ребёнка (`collapse through`). Лист и BFC-бокс работают.

## Как найдено

WPT-RUN-14 срез 13: 28 id в `positioning/`, `abspos/`, `zindex/`, `stacking-context/`, `zorder/` с отрицательным `margin-top`/`margin-bottom` у блока с блочными детьми: `positioning/top-019.xht`…`top-056.xht` (`#div1 { margin-top: -72pt }`, внутри `position:absolute` и `position:relative; top:72pt`), `bottom-*`, `abspos-022.xht`, `relpos-calcs-001/002/007.xht`, `zindex/z-index-001.xht`. Правило отнесения: отрицательный `margin(-top|-bottom)` в стиле теста. Пробой подтверждена на минимальной разметке выше, на самих тестах — `top-019.xht` (`--dump-layout`: `#div1` `m=(-96,…)`, абсолютный потомок остаётся на `y=121.72`).

## Что делать

Схлопывание полей по §8.3.1: результат — наибольшее положительное плюс наименьшее (наибольшее по модулю) отрицательное; отрицательное поле родителя должно участвовать в суммировании так же, как положительное.

## Как проверить

`css/CSS2/positioning/top-019.xht`, `bottom-103.xht`.
