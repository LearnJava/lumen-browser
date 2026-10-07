# BUG-1406 — `mask-image`: `mask-size`/`mask-position`/`mask-repeat`/`mask-composite` не действуют, `url()`-маска не накладывается

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** paint/layout (`crates/engine/layout/src/style/apply/paint.rs` — `mask_layers`; `display_list` — `PushMaskLinearGradient`; `crates/engine/paint/src/cpu_raster.rs` — маски)

## Симптом

`--screenshot`, 800×600, `<div style="width:200px;height:200px;background:green;…">`, зелёных пикселей:

| `style` | получено | ожидается |
|---|---|---|
| `mask-image:linear-gradient(black,black)` | 40 000 | 40 000 |
| то же + `mask-size:50px 50px; mask-repeat:no-repeat` | **40 000** | 2 500 (центр, `mask-position: center`) |
| то же + `mask-position:right bottom` | **40 000** | 2 500 в правом нижнем углу |
| `mask-size:50px 50px; mask-repeat:repeat` / `space` | 40 000 | 40 000 (плитка) |
| два слоя, `100px 100px` и `50px 50px`, `mask-composite:exclude` | **40 000** | 7 500 |
| то же, `mask-composite:intersect` | **40 000** | 2 500 |
| shorthand `mask:linear-gradient(black,black) 0 0/50px 50px no-repeat` | **40 000** | 2 500 |
| `-webkit-mask-image` + `-webkit-mask-size` + `-webkit-mask-repeat` | **40 000** | 2 500 |
| `mask-image:url(transparent-100x50-blue-100x50.png)` (100×100, верхняя половина прозрачна), блок 100×100 | **10 000** | 5 000 |

`--dump-display-list` для двухслойной маски: единственная команда `PushMaskLinearGradient (0.00, 0.00, 200.00, 200.00)
angle=180.0 stops=2` + `PopMask` — размер слоя (`mask-size`) в команду не попадает.

Верно работают: `mask-clip`, `mask-origin`, `mask-mode: luminance`, SVG `<mask>` на inline-SVG
(`<mask><rect x=50 y=50 width=100 height=100 fill=white/></mask>` → 10 000 px, как ожидается).

## Как найдено

WPT-RUN-14 срез 18, `css/css-masking/mask-image/`: 92 reftest (89 `thick`, 3 `thin-only`; 89 содержат `url(`, остальные —
градиент) + 4 (`mask-image-inline-sliced-1`, `mask-under-border-radius`, `mask-image-clip-exclude`, …).

## Что делать

Передавать в команду маски размер плитки, позицию и повтор (в `PushMask*` уже есть `mask-position`, нет `mask-size`
и `mask-repeat`); реализовать композицию слоёв (`add`/`subtract`/`intersect`/`exclude`) вместо одного верхнего слоя
(`CSS-SPECS.md:462`); растровая `url()`-маска — декодированный источник вместо scissor по bbox (оба растра: CPU
`cpu_raster.rs` и wgpu `renderer.rs`).

## Как проверить

`css/css-masking/mask-image/mask-size-contain.html`, `mask-position-1a.html`, `mask-repeat-1.html`,
`mask-composite-1a.html`, `mask-image-1a.html`.
