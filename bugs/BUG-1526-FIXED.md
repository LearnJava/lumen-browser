# BUG-1526 — `justify-self`/`align-self` у блока и у `position:absolute` бокса без заданного размера не сжимают бокс по содержимому и не выравнивают его в оставшемся месте; `unsafe`/`safe` не различаются

**Статус:** FIXED 2026-10-26 (P3); остаток — [BUG-1715](BUG-1715-OPEN.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/block_flow_trampoline.rs:875` — `justify-self` у блока; абсолютно позиционированный бокс — `box_tree/`)

## Симптом

У блока-потомка с `justify-self: start|center|end` без `width` ширина остаётся на всю ширину содержащего блока (ожидается `fit-content` и смещение по свободному месту); с заданной `width` выравнивание работает. У `position:absolute` бокса с `inset:0` и без размера `align-self: start|center|end` оставляет бокс на всю высоту/ширину (должен сжаться по содержимому) — размеры 40 вместо 20; при двух заданных инсетах и явной ширине `justify-self` не двигает бокс (тот же дефект, что [BUG-1481](BUG-1481-FIXED.md)). `unsafe center` на переполняющем боксе должен выходить за начало (−5), у нас 0 — различие `safe`/`unsafe` не реализовано. Сюда же `justify-self` в блоке в составе `inline` (`block-in-inline`), `text-align` как источник `justify-self: normal`. 96 id (65 abspos и 24 блочных testharness: 1 116 из 1 455 сабтестов; 7 reftest) (в основном `abspos/{align,justify}-self-*-{htb,vlr,vrl}-*`: 8 режимов записи × направления).

## Проба

Проба (`--mcp`, контейнер `width:100px`, потомок с `::before{display:block;width:20px;height:20px}` и без собственной `width`; `offsetLeft/offsetWidth`):

| `justify-self` потомка | у нас | ожидается |
|---|---|---|
| `start` | `x=0, w=100` | `x=0, w=20` |
| `center` | `x=0, w=100` | `x=40, w=20` |
| `end` | `x=0, w=100` | `x=80, w=20` |
| `center` при явной `width:20px` | `x=40, w=20` | то же (работает) |
| `position:absolute; left:0; right:0; width:20px; justify-self:center` в контейнере 40 px | `x=0` | `x=10` |
| контейнер 10 px, потомок `width:20px`, `justify-self:unsafe center` / `safe center` | `x=0` у обоих | `-5` / `0` |


## Как найдено

WPT-RUN-14 срез 24: `css-align/abspos/{align,justify}-self-*`, `*-default-overflow-*`, `safe-*-self-*`, `stretch-intrinsic-size-*`, `table-*-self-stretch`; `css-align/blocks/justify-self-*`, `safe-justify-self-*`, `justify-self-auto-margins-2`, `justify-self-block-in-inline`, `justify-self-text-align-2`; `content-distribution/default-overflow-alignment-single-axis-scroll-container`.

## Что делать

В блочной раскладке применять `justify-self` к размеру (`normal`/`stretch` → заполняет, остальное → `fit-content`) и к смещению по оставшемуся месту; для `position:absolute` — выравнивание в области между инсетами после определения размера (общий код с `anchor-center`, BUG-1481); различать `safe` (не выходить за начало) и `unsafe`.

## Как проверить

`css/css-align/blocks/justify-self-htb-ltr-htb.html`, `abspos/align-self-htb-ltr-htb.html`, `abspos/safe-align-self-htb.html`.

## Исправление (2026-10-26, P3)

Общий код выравнивания — `box_tree/self_align.rs` (`inline_alignment`, `aligned_border_start`, `inline_sizing`):

- **Блок в потоке.** `dispatch_box` делает `width: auto` у блока с выравниванием (`justify-self`/`justify-items` родителя ≠ `normal`/`stretch`) `fit-content` (`justify_fits_content`); сдвиг по оси считает уже родитель после раскладки ребёнка (`align_justify_self_child` в горизонтальном потоке, `finish_child` в `vertical_trampoline`) — ему известны направление и режим письма контейнера. `start`/`end` — по контейнеру, `self-start`/`self-end` — по ребёнку, `left`/`right` — физически; `safe` откатывает к `start`, без ключевого слова выход за край разрешён. Явный `stretch` растягивает и ортогонального ребёнка (`UsedSizeOverride`), вертикальный ребёнок вертикального контейнера получает `fit-content` высоту через тот же override.
- **`position:absolute`.** Оси выравнивания берутся у содержащего блока (`place_abs_child` получает его стиль, если это сам родитель), а не у бокса; между двумя инсетами auto-размер — `fit-content`, а не растяжение; default overflow alignment держит margin box внутри объединения CB и inset-modified CB (начальный край побеждает), явный `unsafe` (`ContentAlignExtra::{self_unsafe, justify_self_unsafe}`) — без поправки.
- Проверка: `box_tree/tests/align_self_wpt.rs` — эмуляция `check-layout-th.js` на вендоренных файлах `css-align` (77 файлов целиком), плюс юнит-тесты в `flow_modes.rs` и `abs_auto_margins.rs`.
