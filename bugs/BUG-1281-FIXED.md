# BUG-1281 — высота строки берётся из `line-height` блока, более крупный инлайн-текст строку не раздвигает

**Статус:** FIXED 2026-10-19 (P3)
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

## Исправление (2026-10-19)

`InlineRun` теперь растёт по фрагментам (`BaselineFit`, `box_tree/inline_wrap.rs`): для каждого фрагмента с
`vertical-align: baseline` и шрифтом, отличным от шрифта блока, считаются `ascent + half-leading` над baseline и
`descent + half-leading` под ней (`ascent_px_with_families` — тот же ascent, по которому paint ставит baseline в
прямоугольнике текста). Высота строки рана — максимум по строкам, она же записывается в `used_line_height`
(paint, selection и hit-test умножают индекс строки на него — раскладка остаётся равномерной). Фрагменты разных
кеглей садятся на общую базовую линию вместо центрирования по half-leading. База берётся свежей из стиля, а в
`graft_geometry` значение копируется из `prev` вместе с `lines`. Для `::first-line` и строк вокруг float-ов
(`inline_shape_wrap`) модель прежняя.

Не сделано: неравные строки (строка без крупного фрагмента в многострочном ране получает запас снизу, а не свою
меньшую высоту — раскладка остаётся равномерной) и рост от inline-картинок и `vertical-align: top/bottom/middle`.
Тесты: `box_tree/tests/inline_line_growth.rs`. Эталоны: `55-text-rendering` (строка `.row` с `span 22px` в блоке 16px
стала выше — по §10.8.1), `samples/test-06-layout.html` (span 11px в блоке 13px сел на baseline).
