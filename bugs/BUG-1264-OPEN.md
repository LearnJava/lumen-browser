# BUG-1264 — flex-элементы с другим `writing-mode`, чем у контейнера (ортогональные потоки): размеры по главной/поперечной оси считаются так, будто режимы совпадают

**Статус:** OPEN
**Заведён:** 2026-10-04 (P1, FLEX-VWM)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`, `intrinsic.rs`, `flex_trampoline.rs`)

## Симптом

Flex-элемент с `writing-mode`, ортогональным контейнеру (горизонтальный элемент во вертикальном контейнере и наоборот), раскладывается неверно:

- `inline-size`/`block-size` элемента маппятся на `width`/`height` по режиму элемента, а flex считает главную ось по режиму контейнера — `inline-size: 6px` у `vertical-lr` элемента в горизонтальном ряду должен ограничивать *высоту* (поперечную ось), а flex-basis `auto` — block-size (ширину) по содержимому;
- `max_content_outer_width`/`min_content_outer_width` не знают, что inline-size вертикального бокса идёт по `y`, а вертикальный текст переносится по высоте, — перенос текста внутри ортогонального элемента (`p b c` в 6px-ной «колонке» из WPT) не считается;
- `align-self: stretch` ортогонального элемента не пересчитывает его inline-size.

Примеры: `flexbox-writing-mode-010…016.html` (эталон отличается положением вертикального текста с `direction: rtl` и шириной ортогональных элементов), `stretching-orthogonal-flows.html`,
`percentage-size-subitems-001.html`, `ortho-table-item-001.html`, `grid-flex-item-001.html`, `flex-aspect-ratio-img-vert-lr.html`.

## Описание

FLEX-VWM закрыл оси контейнера (`flex::flex_axes`) и размеры вертикального элемента в *совпадающем* режиме (`max_content_outer_height` — max-content inline-size вертикального блока).
Для ортогональных потоков нужны: перенос текста в вертикальной колонке по inline-size, min-/max-content по обеим осям, пересчёт размера элемента, когда выросла его inline-size (flex stretch), и согласование `direction: rtl` внутри вертикального текста.

## Как найдено

FLEX-VWM, остаток `flexbox-writing-mode-*`: 010–016 не совпадают с эталоном при совпадающих осях контейнера.

## Как проверить

`python .tmp/vwm_ab.py`-подобный прогон reftest по `css/css-flexbox` (см. `tests/wpt/reftest_pixdiff.py`): перечисленные файлы должны стать `identical`.

## Сделано в FLEX-VWM-3 (2026-10-04)

- `flex::column_item_stretches_block_axis`: вертикальный элемент колонки горизонтального контейнера с `width: auto` и `align-self: stretch` получает block-размер (ширину) = поперечный размер колонки; пробный прогон не переигрывается (`flexbox-writing-mode-013`, `stretching-orthogonal-flows`).
- Колоночный элемент с авторским content-box `width` при повторной раскладке (хит пробной памяти, сброшенный вниз float) теряет padding+border — `BorderBox` форсировался и для ширины; теперь ширина передаётся как border-box.
- `place_float`: потомки float-а едут за боксом (`shift_tree`; у `float:right` текст оставался у левого края), `position: relative` у float-а сдвигает и бокс, ширина «по содержимому» отдаётся вместе с margin (`lay_out` вычитает их из `available_width`), вертикальный float с `height:auto` обтягивает содержимое по inline-оси.
- Вертикальный поток (`vertical_trampoline::finish_child`): физические margin по block-оси (`margin-left`/`-right` занимали 0 места), схлопывание смежных margin соседей, `direction: rtl` прижимает over-constrained блок к низу; `bfc::first/last_collapsible_child` не сворачивают margin детей вертикального бокса вверх (дети стоят по x, margin-top ребёнка удваивался).
- `block_flow_trampoline::align_rtl_overconstrained_child` — CSS 2.1 §10.3.3: при `direction: rtl` у родителя блок с заданной шириной прижимается вправо (раньше всегда влево).
- Процентные padding/margin вертикального бокса считаются от inline-размера содержащего блока (`flexbox_align-items-stretch-3`).

Совпали с эталоном: `flexbox-writing-mode-010`, `013`, `014`, `015`, `016`, `flexbox_align-items-stretch-3`; `stretching-orthogonal-flows` — только тонкие линии.

## Сделано в FLEX-VWM-4 (2026-10-04)

- `flex_trampoline::relayout_stretched_row_item`: элемент ряда с `align-self: stretch`, `height: auto` и `height:%`/`calc()` в поддереве раскладывается заново с растянутой высотой линии (CSS Flexbox §9.8 — растянутый элемент определён; §9.4 шаг 11 — и когда первый проход вышел выше линии). `percentage-size-subitems-001`, `percentage-heights-021`, `flex-wrap-006` совпадают с эталоном.
- `intrinsic::flex_item_min_main_width`: таблица-элемент не уже своего min-content при любом `min-width` (CSS Tables L3, «used min width of table») — `ortho-table-item-001`, `table-as-item-fixed-min-width-3`.
- Ортогональный блок в in-flow родителе с неопределённой высотой (`block_flow_trampoline`) и вертикальный flex-контейнер блочного уровня (`flex_trampoline`, высота = размер содержимого по главной оси) обтягивают содержимое по inline-оси вместо заполнения вьюпорта (Writing Modes L3 §7.3.1); корневой элемент по-прежнему заполняет ICB. `flex-direction-row-vertical`, `webkit-box-vertical-writing-mode` (паритет с `display:flex`), `flex-aspect-ratio-img-vert-lr` (остались AA-линии картинки на дробном `y`).
- A/B reftest: css-flexbox identical 463 → 472 (16 лучше, 2 хуже — `aspect-ratio-intrinsic-size-001/002`, оба и раньше не совпадали), css-writing-modes identical 94 → 97; A/B `--dump-display-list` по 182 страницам корпуса — без изменений.

## Остаток

- `dynamic-orthogonal-flex-item` (→ FLEX-VWM-6): ширина shrink-to-fit `inline-flex` считается до раскладки из intrinsic-функций, `vertical_block_extent` берёт ряд `InlineBlockRow` за одну колонку, а flex-проба элемента (`block_axis_width`) число колонок знает — контейнер 50 вместо 100, `flex-shrink` ужимает элемент до min-content одной колонки.
- `aspect-ratio-intrinsic-size-001/002` (→ FLEX-VWM-6): `canvas` с `height:100%` в растянутом элементе — ширина элемента должна браться из растянутой высоты.
- `grid-flex-item-001` (→ GRID-VWM): grid в вертикальном `writing-mode` не меняет оси.
- `flexbox-writing-mode-011/012`: эталон держится на `float: left` в вертикальном потоке (линейный «левый» край — верх). Float в вертикальном потоке сделан (LAYOUT-VFLOAT, 2026-10-05) и строки эталона совпадают с тестом по геометрии первых четырёх элементов, но тест остаётся красным: элементы параллельного режима (`vl`/`vr` в `vl`/`vr`-контейнере) должны растянуться по физической ширине (`align-items: stretch`, 126px у эталона), а flex оставляет им `40px` — это FLEX-VWM-5 (ROADMAP:1017). `shape-outside` в вертикальных режимах (`css-shapes`, ~100 тестов) — LAYOUT-VFLOAT-2.
- Схлопывание margin предка с первым/последним ребёнком по block-оси вертикального потока (`css-writing-modes/margin-collapse-vlr-*`, `-vrl-*`).
