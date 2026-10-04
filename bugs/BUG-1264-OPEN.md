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
