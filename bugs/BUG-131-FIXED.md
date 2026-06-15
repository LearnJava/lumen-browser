# BUG-131

**Статус:** FIXED 2026-06-13
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

INTERACTION TEST-100 (transform×overflow) 9.57%: трансформированный ребёнок (собственный SC) сбегал из overflow:hidden предка — overflow-клип эмитился inline в бакете родительского SC и закрывался ДО отрисовки дочернего SC (отдельный бакет, более поздний слот painting order), оставляя PushClipRect/PopClip пустыми. Fix: fill_buckets переустанавливает rect-клипы (PushClipRect/PushClipRoundedRect) non-SC предков как внешний слой дочернего SC (push в начало pre, парный PopClip после post/CloseLayer); цепочка сбрасывается на каждом SC-anchor. Затрагивает только ordered-билдер (путь shell/femtovg); walk-билдер клиппил inline и был корректен, поэтому cpu_raster snapshot не менялся. Верифицировано --dump-display-list: все 6 ячеек теперь обёрнуты клипами предков; регресс-тест ordered_transformed_child_clipped_by_overflow_hidden_ancestor.
