# BUG-1649 — GPU-бэкенды растягивают растр SVG-фона, а не перерисовывают SVG в размер плитки

**Статус:** OPEN
**Заведён:** 2026-10-19 (P3, остаток [BUG-1296](BUG-1296-FIXED.md))
**Область:** paint (`crates/engine/paint/src/backends/femtovg_backend.rs`, `renderer/` wgpu — `DrawBackgroundImage`)

## Симптом

CPU-растеризатор (`--screenshot`, WPT) с BUG-1296 перерисовывает SVG-фон в размер плитки через `Image::vector_source`. Живое окно (femtovg/wgpu) по коду по-прежнему регистрирует текстуру в собственном размере SVG и масштабирует её GPU-фильтром: у `background-size`, сильно отличающегося от внутреннего, граница между цветами размыта. Вживую не проверено — вывод из чтения кода.

## Что делать

Для `Image` с `vector_source` регистрировать в GPU-кэше текстуру по паре «src, размер плитки на устройстве» (`lumen_image::rasterize_svg`), как это делает `vector_tile` в `cpu_raster.rs`. То же для `DrawImage` (`<img src=*.svg>` при масштабировании).

## Как проверить

`background-size/vector/background-size-vector-001.html` в живом окне против `--screenshot`: граница `lime`/`aqua` резкая в обоих.
