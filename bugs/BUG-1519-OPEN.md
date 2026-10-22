# BUG-1519 — `shape-outside: <image>` (`url()`, `linear-gradient()`, `radial-gradient()`) и `shape-image-threshold` не влияют на обтекание

**Статус:** OPEN (срез 1 влит; ДОРАБОТКА → SHAPE-OUTSIDE-3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout (`crates/engine/layout/src/box_tree/shapes_floats.rs`, `block_flow_trampoline.rs::wire_shape_outside`)

## Симптом

`register_shape_outside` не знает `url(...)`/градиентов: float с такой формой обтекается как прямоугольник. Контур по альфа-каналу изображения (порог `shape-image-threshold`), градиенты, `shape-margin` для изображений не реализованы. `shape-image-threshold` разбирается и хранится (`apply/layout.rs:487`), но нигде не читается.

## Проба

Проба (`--mcp`, float `100×100`, `shape-outside: url(data:image/svg+xml,…<circle r=50/>)`; левые края строк): `100,100,100,100,…` (обтекание прямоугольником); ожидается круг (`90,99,100,99,90,…`). С `linear-gradient(to right bottom, black 50%, transparent 50%)` и `radial-gradient(circle closest-side, black 99%, transparent 100%)` — тоже `100,…`.

## Как найдено

WPT-RUN-14 срез 23: `shape-outside/shape-image/*` (28 id), `shape-image/gradients/*` (19), `spec-examples/shape-outside-010…013`, `-019`, `assorted/float-retry-push-image`; ещё 9 testharness с `Lines positioned properly` (`shape-outside-radial-gradient-001…004`, `spec-examples/shape-outside-010…013`, `-019`). 52 id (43 reftest + 9 testharness), 38 `thick`, 5 `thin-only`.

## Что делать

Задача SHAPE-OUTSIDE-2: растр изображения/градиента → маска по альфа-каналу с порогом, контур по строкам (CSS Shapes 1 §6.2 «Shapes from Images»), `shape-margin` по маске, `image-set()`/`cross-origin` — отдельно.

## Как проверить

`css/css-shapes/shape-outside/shape-image/shape-image-001.html`, `gradients/shape-outside-linear-gradient-001.html`.

## Срез SHAPE-OUTSIDE-2 (2026-10-21, ветка `p1-shape-outside-2`)

Образы работают: `linear-gradient` (точные полигоны), `radial-`/`conic-`/`repeating-*` и `url()` — растровая маска `Prim::Mask` по альфа-каналу с `shape-image-threshold`, `shape-margin` растит маску; `url()` доходит до layout через `Document::set_shape_image` (shell: `report_shape_images`, драйвер: `report_shape_images` в `session.rs`). WPT `shape-image/*` identical 7 → 23 из 29, `gradients/*` 14 → 15 из 17. Остаток (SHAPE-OUTSIDE-3): ряды `inline-block` (`gradients/…-004`), `url()` из скрипта и `image-set()`, корневые `/images/…` под `file://`.
