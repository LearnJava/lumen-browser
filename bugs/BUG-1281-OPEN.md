# BUG-1281 — высота строки берётся из `line-height` блока, более крупный инлайн-текст строку не раздвигает

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3, `css/css-ui`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — ветка `InlineRun`, `b.rect.height = line_count × used_line_height`)

## Симптом

`<div>a <span style="font-size:40px">Test</span> b</div><div>next</div>` (`--dump-layout`):
`InlineRun rect=(8, 8, 1008, 17.72)`, следующий блок начинается с `y = 25.72`. 40-пиксельный «Test» рисуется
(`DrawText … 40.00`) и на снимке налезает на `next`. Ожидается строка высотой не меньше line-height инлайна
(CSS 2.1 §10.8: высота line box — от верха самого высокого до низа самого низкого inline box строки).

То же с `line-height: 60px` у `<span>` и с `font-size: 30px` в блоке `font-size: 10px` (высота 11.07 вместо ≈34).
У `display: inline-block` — верно (`InlineBlockRow` 44.30): ошибается только текстовый ран.

## Причина

Высота `InlineRun` = `line_count * step_line_height(b.used_line_height, …)` (`layout_dispatch.rs`, конец ветки
`InlineRun`), и `apply_inline_vertical_align(lines, line_h)` получает тот же `line_h` для всех фрагментов. Метрики
сегментов (`seg.style.font_size`/`line_height`) в высоту строки не входят. Многострочный ран с крупным фрагментом
в одной строке так же складывает все строки одной высоты.

## Что делать

Считать высоту каждой строки по её фрагментам (strut блока + `line-height` каждого инлайн-сегмента с учётом
`vertical-align`, §10.8.1) и складывать строки по фактическим высотам; раскладка baseline внутри строки — от неё же.

## Как проверить

WPT `css/css-ui/text-overflow-001…004.html`, `text-overflow.html` (Ahem 30px внутри блока 10px — 5 reftest; их
дополнительно держит [BUG-1273](BUG-1273-FIXED.md), Ahem в reftest не грузится). Форма общая — `css/CSS2/linebox`,
`css/css-inline` не прогонялись.
