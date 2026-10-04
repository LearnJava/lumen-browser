# BUG-1258 — Flex-контейнер с вертикальным `writing-mode` (`vertical-rl`/`vertical-lr`/`sideways-*`) раскладывается как горизонтальный: главная ось `row` остаётся 

**Статус:** FIXED 2026-10-04 (P1, FLEX-VWM; остаток — BUG-1263, BUG-1264, BUG-1265)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`, `vertical_trampoline.rs`)

## Симптом

Flex-контейнер с вертикальным `writing-mode` (`vertical-rl`/`vertical-lr`/`sideways-*`) раскладывается как горизонтальный: главная ось `row` остаётся горизонтальной (проба: `writing-mode:vertical-rl;display:flex`, два ребёнка 30×40 — оба `y=0`, `x=70` и `x=40`, то есть справа налево по горизонтали; ожидается сверху вниз по вертикали, `x=70` у обоих), а флаги осей не пересчитываются через writing-mode/direction. `flex.rs` не упоминает `writing_mode` вовсе. Семейство — физические/логические оси, `flex-direction`, auto-margins, abspos static position, baseline во всех 8 сочетаниях writing-mode×direction. WPT-RUN-14-S1: 116 id по имени/исходнику (`writing-mode: vertical|sideways`, `-vert`, `-vlr`, `-vrl`, `wmvert`, `flexbox-writing-mode-*`), 963 упавших сабтеста — крупнейший кластер.

## Описание

ДОРАБОТКА по `docs/probe-method.md` §8: `grep -ci "writing_mode" crates/engine/layout/src/box_tree/flex.rs` = 0, объём — модель осей. Задача — `FLEX-VWM` в `ROADMAP.md`.

## Как найдено

WPT-RUN-14 срез 1.

## Исправлено

FLEX-VWM (2026-10-04). Раскладка flex-контейнера идёт в «стартовых» координатах, оси берёт `flex::flex_axes`
(`flex-direction` × `writing-mode` × `direction` × `wrap-reverse`), а `flex_trampoline::mirror_reversed_axes` зеркалит поля margin box
вдоль главной/поперечной оси, когда их старт у нижнего/правого края. Вертикальный контейнер больше не падает в блочную раскладку
`vertical.rs`: высота (inline-size) определённая, ширина (block-size) при `width: auto` — по содержимому (`VerticalFlex`).
Попутно: `*-inline|block-*` отображаются на физические стороны по writing-mode/direction (`style/logical.rs`); `safe`/`left`/`right`/`start`/`end`
у `justify-content`/`align-content`/`align-self` (`ContentAlignExtra`); `column-gap`/`row-gap` выбираются по flex-direction, а не по физической оси;
вертикальный блок принимает `UsedSizeOverride` flex-элемента и клампит `min-`/`max-width`; flex-basis вертикального элемента — max-content его inline-size.

Замер (reftest + `check-layout-th`, бинарь до/после): css-flexbox — 919 reftest: 435 → 470 совпадений (36 новых, 1 «регрессия» — эталон `align-baseline.html` сам рисуется неверно); 368 testharness-id: 1746 → 2922 прошедших сабтеста
из 4696 (основная часть — статическая позиция abspos, см. BUG-1251); `graphic_tests/`+`samples/` (182 страницы) — display list совпадает побайтно.

## Остаток

- [BUG-1263](BUG-1263-OPEN.md) — `inline-block`/`<br>` не текут вертикально внутри вертикального блока (`css-flexbox-row*.html`, `flex-item-vertical-align`).
- [BUG-1264](BUG-1264-OPEN.md) — ортогональные flex-элементы (`flexbox-writing-mode-010…016`, `stretching-orthogonal-flows`, `percentage-size-subitems-001`).
- [BUG-1265](BUG-1265-OPEN.md) — `self-start`/`self-end` и `safe` у выравнивания самих items.
