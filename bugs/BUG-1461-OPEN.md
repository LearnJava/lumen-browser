# BUG-1461 — `tab-size: <number>`: шаг табуляции считается по стилю сегмента, а не блочного контейнера

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, остаток [BUG-1326](BUG-1326-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — `TabStops::of(&seg.style, m)`; `inline_wrap_preserved.rs`)

## Симптом

CSS Text L3 §4.2: число в `tab-size` — кратное ширине пробела **ближайшего блочного контейнера** (шрифт, `letter-spacing`, `word-spacing` берутся у него, а не у инлайна с табуляцией). `TabStops::of` берёт их у стиля сегмента, поэтому `<div style="white-space:pre"><span style="tab-size:4;font-size:2em;letter-spacing:5px;word-spacing:5px">\tX</span></div>` даёт шаг из метрик `span`, а ожидается — из метрик `div`. Значение `tab-size` при этом берётся с инлайна (`tab-size-integer-004`: «tab-size applies to inline boxes»).

Выведено из кода, WPT-прогон не делался.

## Что делать

Передать в `wrap_inline_run` ширину пробела (с интервалами) блочного контейнера и использовать её в `TabStops::of` вместо метрик сегмента. Сигнатура `wrap_inline_run` вызывается из ~10 мест (в основном тесты), поэтому лучше положить метрики в одну структуру-параметр.

## Как проверить

`css/css-text/tab-size/tab-size-integer-004.html`, `tab-size-integer-005.html`.
