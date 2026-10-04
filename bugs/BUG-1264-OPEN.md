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

## Остаток

- `dynamic-orthogonal-flex-item`: shrink-to-fit `inline-flex` вокруг ортогонального элемента — число колонок зависит от высоты контейнера, которую intrinsic-функции не знают.
- `percentage-size-subitems-001`: высота `100%` у детей растянутого по поперечной оси flex-элемента (overflow: scroll) — растянутый размер определён, но раскладка элемента не переигрывается с ним.
- `grid-flex-item-001`, `ortho-table-item-001`: `grid`/`table` в вертикальном `writing-mode` не поддержаны.
- `flex-aspect-ratio-img-vert-lr`: размеры скриншотов отличаются.
- `flexbox-writing-mode-011/012`: эталон держится на `float: left` в вертикальном потоке (линейный «левый» край — верх), а float-ы в вертикальных контекстах движок пока игнорирует (`vertical.rs`, «Limitations»); то же для 100+ тестов `css-writing-modes`/`css-shapes`.
- Схлопывание margin предка с первым/последним ребёнком по block-оси вертикального потока (`css-writing-modes/margin-collapse-vlr-*`, `-vrl-*`).
