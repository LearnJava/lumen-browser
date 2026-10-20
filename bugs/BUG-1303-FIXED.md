# BUG-1303 — Плитки `background-image` рисуются только внутри позиционирующей области: при `background-origin: padding-box`/`content-box` полоса между ней и областью рисования (`border-box`) остаётся пустой

**Статус:** FIXED 2026-10-20 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` — `DrawBackgroundImage`, расчёт сетки плиток; отдельно проверена только CPU-ветка)


## Симптом

`<div style="width:100px;height:100px;padding:20px;border:20px solid rgba(0,0,255,.2);background:url(50x50-green.png);background-origin:O">` (повтор по умолчанию, область рисования `border-box`), `--screenshot`, пиксель в рамке (5, 5):

| `O` | получено | ожидается |
|---|---|---|
| `border-box` | `(0,82,92)` — зелёный под рамкой | верно |
| `padding-box` | `(163,163,255)` — только рамка по белому | зелёный под рамкой (плитки продолжаются до края области рисования) |
| `content-box` | `(163,163,255)`, и в зоне padding (30, 30) — белый | то же: зелёный и под рамкой, и в padding |

Без рамки/padding (`background-position: 30px 30px`, origin по умолчанию) плитки назад рисуются верно. Плитки не выходят за **позиционирующую** область, хотя `background-clip` (по умолчанию `border-box`) требует закрасить всю область рисования (CSS Backgrounds 3 §3.4: «the image is repeated … to cover the background painting area»).

## Как найдено

WPT-RUN-14 срез 6: `background-origin/origin-{border,content,padding}-box*.html` — 10 reftest, `css3-background-origin-*.html` (3) и `background-origin-008.html` — `thick`. Глазом (`origin-content-box.html`): во втором блоке (повтор) нет первого ряда/колонки плиток — они остались белыми там, где у эталона заливка.

## Что делать

В `DrawBackgroundImage` (`crates/engine/paint/src/cpu_raster.rs`, wgpu — `renderer.rs`; независимые реализации) генерировать плитки от `origin − k·tile` по обеим осям до края **области рисования**, а не позиционирующей; `space`/`round` — пересчитывать число плиток по позиционирующей области (§3.4/3.5), клипуя по области рисования. Живое окно (wgpu) не проверялось.

## Как проверить

`css/css-backgrounds/background-origin/origin-content-box.html`, `origin-padding-box_with_size.html`, `css3-background-origin-content-box.html`.


## Исправление (2026-10-20, P3)

Сетка плиток якорится к позиционирующей области (`background-origin`), а `repeat_axis_geometry` отдавала первую плитку не левее её края; полоса painting area до origin box оставалась пустой. Новый помощник `extend_start_to_paint_area` (`display_list/geometry.rs`) сдвигает старт повторяющейся оси назад на целое число шагов до края painting area. Применён во всех трёх независимых реализациях: CPU (`cpu_raster::rasterize_background_image`), wgpu (`renderer.rs`, `DrawBackgroundImage`) и femtovg (`image.rs::draw_background_image`). Не повторяющиеся оси не затрагиваются; `space` — тоже периодичен, продолжается тем же шагом.

Тесты: `cpu_raster::tests::background_repeat_covers_strip_before_origin_box`, `display_list::text_and_images::extend_start_to_paint_area_steps_back_to_cover_area`. Эталоны `cpu/*.png` не менялись (страницы с `background-origin` в корпусе — `no-repeat`). Живое окно (wgpu) глазом не проверялось.
