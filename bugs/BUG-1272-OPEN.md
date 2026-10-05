# BUG-1272 — RTL-фрагмент с латиницей и ивритом рисуется задом наперёд: CPU-растр шейпит его как RTL-прогон второй раз

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 2 — `css/css-writing-modes`, кластер bidi)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs::rasterize_text` — вызов `active_text_shaper().shape(…, ShapeDirection::LeftToRight, None, …)`)

## Симптом

`<span style="direction:rtl; unicode-bidi:bidi-override">abc &#x5d0;</span>` (и `<bdo dir=rtl>`) рисуется как `abc א`, а должно — `א cba`.
Без иврита в том же фрагменте (`abc x`) — верно: `x cba`.

Раскладка и display list верны: `--dump-display-list` даёт `DrawText … "א cba"` — фрагмент уже развёрнут
`lumen_layout::bidi::visual_text` (UAX #9 L2) и мирорирован, как требует контракт
`display_list/text_run.rs:147` («rasterizers advance strictly left to right and do no bidi work of their own»).
Пиксели — `abc א`.

## Причина

`rasterize_text` передаёт в `rustybuzz` направление `LeftToRight`, но не скрипт (`None`). `rustybuzz::shape` сам
угадывает скрипт по первому символу с не-Common скриптом (`guess_segment_properties`), а `ensure_native_direction`
(`rustybuzz-0.14.1/src/hb/ot_shape.rs:686`) при горизонтальном направлении, не совпадающем с «родным» направлением
скрипта, разворачивает буфер. В строке `"א cba"` первый такой символ — `א` (Hebrew, родное RTL), поэтому буфер
переворачивается обратно в логический порядок. Строка из одной латиницы (`"x cba"`) скрипт Latin получает, разворота нет.

Итог: любой RTL-фрагмент, в котором первым буквенным символом идёт символ RTL-скрипта (иврит, арабский), а рядом есть
латиница или цифры, рисуется в логическом порядке. Чистый ивритский фрагмент развёрнут дважды и тоже стоит в
логическом порядке.

## Как воспроизвести

```html
<div style="font:30px sans-serif"><span style="direction:rtl;unicode-bidi:bidi-override">abc &#x5d0;</span></div>
```

`lumen --viewport 400x80 --screenshot out.png <файл>` → `abc א`; `--dump-display-list` → `"א cba"`.

## Что делать

Явно задать скрипт буфера, не дающий разворота (`Zyyy`/Latin), либо прогонять `ensure_native_direction`
по уже визуальной строке так, чтобы разворот не случался: растеризатор получает визуальный порядок и обязан рисовать его
слева направо. Проверить те же вызовы в `cpu_raster.rs:3037` (вертикальный upright-путь) и `varied_text.rs:172`,
и wgpu-путь (`renderer/glyph_raster.rs`) — он шейпит иначе, но контракт у него тот же.
Двигает пиксели текста — полный `python graphic_tests/run.py --continue-on-fail`.

## Как проверить

WPT `css/css-writing-modes/bidi-*`, `block-*`/`inline-*` с `embed|override|isolate|plaintext|normal|unset` (94 reftest,
кластер в `docs/wpt-vendor-notes/css.md` §css-writing-modes); пример — `bidi-override-001.html`: обе строки должны
совпасть с `reference/bidi-override-001.html`.
