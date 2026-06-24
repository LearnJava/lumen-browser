# BUG-220

**Статус:** FIXED 2026-06-24
**Компонент:** paint (`crates/engine/paint/src/display_list.rs`)
**Тест:** —  (обнаружено при разборе BUG-202; визуальный эффект мал — overlay scrollbar)

## Исправление

Логика scrollbar (gutter из `scrollbar-width`, цвета из `scrollbar-color`,
`scrollbar_rects`) вынесена в общий хелпер
`emit_scrollbars(b, padding_box, is_scroll_x, is_scroll_y, out)` и вызывается из
обеих веток построения display-list-а:

- `walk` (legacy путь) — после `PopScrollLayer`;
- `box_layer_ops` (ordered / stacking-context путь) — `DrawScrollbar` пушится в
  `overflow_post` сразу после `PopScrollLayer`; caller (`fill_buckets`) сбрасывает
  `overflow_post` после детей (в `bucket.post` для SC-root, в `bucket.contents`
  для non-SC), поэтому бары рисуются на фиксированной позиции поверх
  проскролленного контента.

Хелпер измеряет content-extent относительно **padding-box** origin и с полом
**padding-box** размера (а не border-box) — это устраняет описанный ниже риск
ложного горизонтального scrollbar при наличии border.

Проверка: `--dump-display-list 83-scroll-behavior.html` теперь даёт 3
`DrawScrollbar` на 3 `PushScrollLayer` (было 0). Регресс-тест
`ordered_scroll_container_emits_scrollbar` в `display_list.rs`. TEST-83 остаётся
KNOWN_DEBTOR (BUG-128, font-parity): добавленные полупрозрачные бары (~0.3–0.8%
площади) в пределах ратчет-полосы baseline 7.88% ±2%.

## Описание

Scroll-контейнер (`overflow: scroll/auto` с переполнением), который рисуется через
ordered (stacking-context) путь `build_display_list_ordered` → `box_layer_ops`,
**не получает `DrawScrollbar`**. `box_layer_ops` (display_list.rs:2481) эмитит
`PushScrollLayer`/`PopScrollLayer`, но scrollbar track+thumb эмитятся только в
legacy `walk` (display_list.rs:5152). Подтверждено `--dump-display-list` на
TEST-83: для трёх `.scroll-box` есть `PushScrollLayer`, но ноль `DrawScrollbar`.

## Воспроизведение

```bash
lumen.exe --dump-display-list graphic_tests/83-scroll-behavior.html | grep -i scrollbar
# (нет DrawScrollbar, хотя есть PushScrollLayer)
```

## Как чинить

Вынести логику scrollbar из `walk` (gutter из `scrollbar-width`, цвета из
`scrollbar-color`, `scrollbar_rects`) в общий helper `emit_scrollbars(b,
padding_box, is_scroll_x, is_scroll_y, out)` и вызвать его из обеих веток:
`walk` (после `PopScrollLayer`) и `box_layer_ops` (push в `overflow_post` после
`PopScrollLayer`). Внимание: content_w/content_h надо мерить относительно
padding-box origin и пола padding-box размера, иначе появляется ложный
горизонтальный scrollbar (content шире clip на ширину border).

**Замечание:** Edge на Windows показывает overlay-scrollbar, который скрыт в
статическом скриншоте, поэтому отрисовка scrollbar **немного увеличивает** diff с
эталоном (см. TEST-83). Перед фиксом — решить, рисовать ли scrollbar статически
вообще (consistency с `walk`) или прятать (parity с Edge).
