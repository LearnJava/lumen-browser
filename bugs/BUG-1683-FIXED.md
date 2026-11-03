# BUG-1683 — Полосы прокрутки под-документов (`<iframe>`) смещены на высоту тулбара

**Статус:** FIXED 2026-10-28
**Заведён:** 2026-10-22 (найден при BUG-1642)
**Область:** shell (`crates/shell/src/frames.rs::frame_scrollbar_overlay`, `crates/shell/src/lumen/scrolling.rs::frame_local_point`)

## Симптом

Overlay-буфер рисуется в координатах окна, страница — со сдвигом `page_offset()` (под тулбар,
`CHROME_H` = 72 px). `frame_scrollbar_overlay` кладёт полосу фрейма в страничные координаты
хоста (`frame_box_page_origin` − scroll страницы) без этого сдвига, `frame_local_point` так же
хит-тестит сырые оконные координаты. В живом окне полоса фрейма лежит на 72 px выше самого фрейма,
клик по ней попадает не туда. Страничные полосы выправлены в BUG-1642 (`scrollbar::shift_overlay_y`,
`Lumen::page_local_y`).

## Что сделать

Сдвинуть `sx/sy` в `frame_scrollbar_overlay` на `page_offset()` и вычитать его в
`frame_local_point`; тест на пару «рисуем — попадаем кликом».
