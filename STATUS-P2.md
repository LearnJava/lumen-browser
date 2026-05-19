In progress: —

Next:
1B CompositorThread struct + spawn loop            paint/src/compositor.rs:277
1B vsync tick-loop 60fps                           paint/src/compositor.rs (after CompositorThread)
1B PushBlendMode/PopBlendMode in build_display_list  paint/src/display_list.rs:196
1B off-screen opacity layer rendering              paint/src/renderer.rs
1B GPU texture upload for layer snapshots          paint/src/renderer.rs
3A ColorSpace enum in ComputedStyle                layout/src/style.rs:1159
3A Display P3 parsing in CSS color functions       layout/src/style.rs:9919
3A HDR tone-mapping utilities (sRGB↔P3 matrices)  layout/src/style.rs
3A ColorFloat variant (f32 channels)               layout/src/style.rs:494
3A color space awareness in renderer               paint/src/renderer.rs

Blocked:
3A color management — needs P1 1B Color type first
3B animations offload — needs P1 1B + P3 scheduling

Recent: graphic-tests-rework 2026-05-19
