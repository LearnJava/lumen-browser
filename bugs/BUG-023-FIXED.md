# BUG-023

**Статус:** FIXED 2026-05-26
**Компонент:** layout+paint

## Описание

opacity deviation — P1: strut fix 2026-05-26; P5 paint: premultiplied alpha double-mult at edge-AA pixels in composite shader → TEST-13 0.24%

## Детали

Opacity compositing математически корректен: `PushOpacity`/`PopOpacity` + off-screen layer composite shader (`c.rgb * in.alpha + white * (1 - in.alpha)`). TEST-13 (2.20%) не хуже TEST-02 color-named (2.35%) без opacity — т.е. opacity не добавляет ошибку.

**P1-часть FIXED 2026-05-24** (commit на ветке p1-bug-023-strut): InlineBlockRow больше не добавляет strut_descent в строках без InlineRun. Edge/Blink не расширяют line box font-strut'ом, когда в строке только inline-block/replaced элементы; ранее каждый такой ряд накапливал ~3.86 px (Inter, font-size:16) лишнего descender, смещая последующие блоки.

Оставшиеся ~1.6% — edge antialiasing: Edge сглаживает рёбра, Lumen нет. Для снижения ниже 1% — MSAA/SSAA в renderer (P2).

**Статус после фикса:** остаточный sub-1% edge-AA — TEST-13 0.24%.
