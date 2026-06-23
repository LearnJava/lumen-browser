# BUG-238

**Статус:** FIXED 2026-06-23
**Компонент:** layout (css-parser) + paint
**Файлы:** `crates/engine/layout/src/style.rs`, `crates/engine/paint/src/display_list.rs`,
`crates/engine/paint/src/backends/femtovg_backend.rs`, `crates/engine/paint/src/cpu_raster.rs`

## Описание

`radial-gradient(ellipse …)` рисовался **кругом**, а не эллипсом. CSS-парсер
(`parse_background_gradient`) извлекал только центр градиента и отбрасывал
ключевые слова формы (`circle`/`ellipse`) и размера (`closest-side` …
`farthest-corner`). Команда дисплей-листа `DrawRadialGradient` несла лишь центр,
поэтому все бэкенды рисовали изотропный круг радиусом «farthest-corner»
(`hypot(dx, dy)`). Для `ellipse`-градиента в неквадратном боксе (TEST-39
`.rg-ellipse`, 240×120) это давало сильное расхождение с Edge: круг радиуса 134px
вместо эллипса 169.7×84.85.

Найдено при D2-1 (BUG-085): per-box diff TEST-39 показал, что `.rg-ellipse` даёт
**69%** всего расхождения теста (8927 из 12939 «плохих» пикселей при пороге 16) —
это был доминирующий дефект, а не квантизация градиент-текстуры.

Предыстория: BUG-221 намеренно сделал radial **кругом** во всех бэкендах, чтобы
CPU-снимок совпадал с femtovg. Но это было верно только для `circle`-градиентов;
единственный `ellipse` в наборе тестов рисовался неправильно.

## Исправление

Сквозная поддержка формы/размера радиального градиента (CSS Images L3 §3.5):

1. **`style.rs`**: enum `RadialShape { Circle, Ellipse }` + `RadialSize`
   (4 extent-ключевых слова); поля `shape`/`size` в `ParsedGradient::Radial`;
   парсер `parse_radial_gradient_shape_size` (default shape = ellipse,
   default size = farthest-corner, по спеку); функция `radial_gradient_radii`
   резолвит форму/размер в `(rx, ry)` px против бокса (corner-размеры используют
   aspect-ratio совпадающего side-размера и проходят через нужный угол — §3.5.1).
2. **`display_list.rs`**: `DrawRadialGradient` получил `radius_x`/`radius_y` (px);
   билдер вычисляет их через `radial_gradient_radii` под каждый paint-rect.
3. **femtovg**: `draw_radial_gradient_cpu` теперь сэмплит эллиптическую дистанцию
   `sqrt((dx/rx)² + (dy/ry)²)` (круг = `rx == ry`).
4. **cpu_raster**: эллипс через `RadialGradient` + вертикальный scale `ry/rx`
   вокруг центра (круг `rx == ry` → identity → byte-identical, BUG-221 сохранён).
5. **wgpu renderer**: оставлен как был (не дефолтный бэкенд; игнорирует новые поля).

`circle`-градиенты рендерятся байт-в-байт как раньше (rx == ry). Default shape
сменился на `ellipse` (спек), но во всём наборе тестов радиальные градиенты
используют явный `circle`, кроме `.rg-ellipse` — регрессий нет.

Проверено: TEST-39 `.rg-ellipse` 31.0% → **0.76%** (по пикселям бокса, порог 16);
TEST-39 в целом 1.62% → **1.18%**. TEST-49 (radial circle + transparent-fade)
PASS 0.47%, TEST-26 (radial mask) PASS 0.00% — круги не задеты. +4 unit-теста
(`radial_radii_*`, `radial_shape_size_parses_*`).
