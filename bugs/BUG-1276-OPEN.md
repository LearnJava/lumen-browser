# BUG-1276 — в вертикальном `writing-mode` не работают смещения позиционированных боксов: инсеты `absolute` и сдвиг `relative` игнорируются

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 2 — `css/css-writing-modes`, крупнейший кластер среза)
**Область:** layout (`crates/engine/layout/src/box_tree/vertical_trampoline.rs` — не зовёт `layout_dispatch::finish_after_match`)

## Симптом

Containing block — `position: relative; writing-mode: vertical-lr; 320×320`, ребёнок — `position: absolute; 50×50`
(`--dump-layout`, CB в `(8, y0)`):

| инсеты ребёнка | получено | ожидается |
|---|---|---|
| `top: 10px` | `(8, y0)` | `(8, y0+10)` |
| `bottom: 10px` | `(8, y0)` | `(8, y0+260)` |
| `left: 10px` | `(8, y0)` | `(18, y0)` |
| `right: 10px` | `(8, y0)` | `(268, y0)` |
| `top: 10px; bottom: 20px` | высота 320 | высота 290 |

Тот же ребёнок в горизонтальном CB или с собственным `writing-mode: vertical-*` в горизонтальном CB — верно. Решает
режим **родителя**: CB `vertical-lr` с ребёнком `writing-mode: horizontal-tb` тоже даёт `(8, y0)`.

`position: relative; top: 10px; left: 20px` у блока внутри `vertical-lr` — смещения нет (`(0, 0)` вместо `(20, 10)`).

Неинсетная часть работает: статическая позиция abspos-ребёнка в вертикальном потоке верна (`abs-pos-non-replaced-vlr-*`
ставят его по статике, ошибается только всё, что зависит от инсетов и `auto`-размеров по ним).

## Причина

Горизонтальные ветки диспетчера заканчиваются `finish_after_match` (`layout_dispatch.rs:1838`): там
`lay_out_abs_children` (CSS Position L3 §4) для отложенных abspos-детей и сдвиг `position: relative` (§9.4.3).
Вертикальная ветка (`DispatchOutcome::NeedsVerticalLoop` → `vertical_trampoline::run`) этот хвост не вызывает:
abspos-дети раскладываются как обычные дети потока, а относительный сдвиг не применяется.

## Что делать

Вызвать из `vertical_trampoline::finish_frame` тот же хвост: отложить abspos-детей (как `BlockFlowInit::abs_deferred`)
и разместить их против padding box CB с инсетами по физическим сторонам (CSS Writing Modes L3 §7.1: правила §10.3.7 /
§10.6.4 CSS 2.1 применяются к осям по режиму CB — для `vertical-*` «горизонтальное» уравнение решается по высоте), затем
сдвиг `relative`. `direction: rtl` в вертикальном CB меняет, к какому краю прижимается статическая позиция.

## Как проверить

WPT `css/css-writing-modes/abs-pos-non-replaced-*`, `abs-pos-border-offset-*`, `box-offsets-rel-pos-*`,
`overconstrained-rel-pos-*`, `dynamic-offset-*` — 259 reftest (`docs/wpt-vendor-notes/css.md` §css-writing-modes).
Эталоны части других тестов (`sizing-orthog-*-ref.xht`) сами ставят блоки `position: absolute` внутри вертикального
корня — после правки сдвинутся и они.
