# BUG-220

**Статус:** OPEN
**Компонент:** paint (`crates/engine/paint/src/display_list.rs`)
**Тест:** —  (обнаружено при разборе BUG-202; визуальный эффект мал — overlay scrollbar)

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
