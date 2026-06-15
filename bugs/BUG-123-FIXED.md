# BUG-123

**Статус:** FIXED 2026-06-11
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

scroll/overflow container's own background+border clipped by its own overflow clip: `box_layer_ops` put PushScrollLayer/PushClipRect (scissor = padding-box) into `pre`, and `fill_buckets` emitted `emit_box_self` (bg/border) AFTER all pre-ops → 2px border fully outside scissor, background inset 2px per side. TEST-51 diff 1.39% was exactly this (masked by BUG-093 threshold raise). Per CSS Overflow L3 §3.2 overflow clips children only; non-compositor `walk` already did it right. Fix: `BoxLayerOps` struct splits effect ops (clip-path/blend/opacity/transform/filters — wrap bg/border) from overflow clip (wraps children only); emission order pre → bg/border → overflow_pre → children → overflow_post → post. Regression test `ordered_scroll_container_bg_border_outside_scroll_layer`. TEST-51: 1.39% → 1.09%
