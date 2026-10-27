# BUG-1733 — `backdrop-filter`: остаток после BUG-1539 (корни фона предков, 3D, режимы края, живое окно)

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1539](BUG-1539-FIXED.md))
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs::apply_backdrop_to_stack`, wgpu `crates/engine/paint/src/renderer.rs` `PushBackdropFilter`)

## Симптом

После BUG-1539 на CPU-пути (`--screenshot`) `backdrop-filter` работает под собственными `opacity`, `transform` (аффинным), `clip-path`, `mix-blend-mode`, `border-radius` и сквозь `isolation: isolate` предка. В `css/filter-effects/backdrop-filter-*` (800×600, `tests/wpt/reftest_pixdiff`) остаются красными (`thick`):

- корни фона у предков: `backdrop-root-opacity`, `-filter`, `-mask`, `-mix-blend-mode`, `-clip-path`, `-clip-path-2` (последний — ещё и `display: inline` с `clip-path`, `PushBackdropFilter` в дампе вовсе нет);
- вложенные скругления: `nested-border-radius-clip`, `-2`, `-3`, `-4`; `clip-radius-zoom`, `clip-rect-2`;
- 3D: `3d-transform-perspective`, `nested-3d-transform-perspective` (проективный образ рамки на CPU не моделируется — `apply_backdrop_to_stack` выходит);
- режимы края и размеры: `edge-behavior`, `edge-mirror`, `edge-pixels-2`, `edge-clipping`, `-2`, `blur-large-value`, `boundary`, `basic-blur`;
- прочее: `svg*`, `reference-filter*`, `fixed-clip`, `isolation-fixed`, `inline-positioning`, `transform`, `scale-transform`, `image-size-filter-size-mismatch`, `invalid`, `plus-filter`, `opacity`/`opacity-rounded-clip`;
- `isolation-isolate` — различие в 1 уровень канала (округление композита `Opacity(1.0)`), помечается как `thick` по счёту пикселей.

Отдельно: wgpu-путь (живое окно) для групп элемента не проверялся; `radii` и `wrappers` команды он не читает.

Анимационный кластер (`css-backdrop-filters-animation-*`) — [BUG-1234](BUG-1234-OPEN.md), если запись ещё открыта.

## Что делать

Разбирать по группам выше, начиная с корней фона у предков и скруглений (самые дешёвые: уже есть стек слоёв и маска). Проверка: `tests/wpt/reftest_pixdiff.classify` по `/css/filter-effects/*backdrop*` до/после.
