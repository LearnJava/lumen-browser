# BUG-109

**Статус:** FIXED 2026-06-23
**Компонент:** paint/font
**Файл:** `crates/engine/paint/src/varied_text.rs`, `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

`font-variation-settings` axis values (wght/wdth/slnt/…) не применялись при
рендеринге в дефолтном femtovg-окне: femtovg отдаёт текст собственному шейперу,
у которого нет API для variation-координат, поэтому варьируемые run-ы рисовались
в default-instance шрифта.

GPU-путь (`renderer.rs::push_text_glyphs` → `normalize_variation_axes` →
`Font::glyph_resolved_with_coords`) применял оси и раньше — дефект был только в
femtovg-бэкенде.

Исходная метрика «TEST-68: 3.21%» устарела: тест переписан на пустые цветные
боксы («purely geometric, no glyph rasterization diff»), сейчас PASS (0.11%),
так что pixel-diff он этот дефект не ловит.

## Исправление

1. Новый модуль `crates/engine/paint/src/varied_text.rs`:
   `build_varied_text_paths()` разрешает каждый глиф через
   `Font::glyph_resolved_with_coords` в запрошенной точке пространства осей и
   эмитит backend-агностичные `PathCmd` (move/line/quad/close) в экранных
   пикселях. Обход контура зеркалит `lumen_font::rasterizer::walk_contour`
   (квадратичные кривые, on/off-curve, неявные midpoint-ы). Возвращает `None`
   для статических фейсов — там вариация не влияет, рендер уходит на нативный
   путь бэкенда.
2. `femtovg_backend::draw_varied_text()` (вызывается из `render_command`, когда
   `font_variation_axes` непустой): находит первое CSS-семейство,
   разрешающееся в **variable** face, строит пути и заливает их цветом текста
   через `Canvas::fill_path` — с учётом текущего transform/clip/AA канвы.
   Если variable-фейс не найден (нет провайдера, только статические/generic
   семейства) — `false`, и текст рисуется штатным femtovg-путём.

CPU-растеризатор (`cpu_raster.rs`) не трогали: он использует только bundled
Inter-Regular (статический, без `fvar`), поэтому прокидка осей там была бы
no-op, а детерминированный snapshot-baseline остаётся неизменным.
