# BUG-1436 — Вырожденный `repeating-radial-gradient` (огромная позиция центра) обрывает весь `--screenshot` ошибкой вместо пропуска градиента

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images` + `css/css-values` + `css/css-color`)
**Область:** paint (`--screenshot` / CPU-растр; ошибка «degenerate radial gradient»)

## Симптом

`lumen --viewport 800x600 --screenshot out.png file:///…/css-images/infinite-radial-gradient-refcrash.html`:

```
Ошибка --screenshot …/infinite-radial-gradient-refcrash.html: degenerate radial gradient
```

PNG не создан. Тот же снимок эталона `infinite-radial-gradient-crash-ref.html` (простой `div` без градиента) — `Снимок сохранён`.

## Как найдено

WPT-RUN-14 срез 19: `reftest_pixdiff.py` — `shot-failed 1`; `infinite-radial-gradient-refcrash.html` под `wptrunner` — FAIL.

## Что делать

Вырожденный градиент (центр/радиус нечисловые или бесконечные после перевода процента) — рисовать как пустой слой или как сплошной
цвет последней остановки (CSS Images 3 §3.2.2: «degenerate» → цвет последней остановки), но не прерывать снимок всей страницы.

## Как проверить

`lumen --screenshot out.png file:///…/css-images/infinite-radial-gradient-refcrash.html` — PNG создан.

## Исправление

Корень: центр `at 9999…9%` (100 девяток) переполняет f32 в `inf`, `RadialGradient::new` в tiny-skia отказывает, а `rasterize_radial_gradient` (`crates/engine/paint/src/cpu_raster.rs`) превращал отказ в `Err` — весь `--screenshot` падал.
Теперь неконечный центр/радиус заливает бокс цветом последней остановки (каждый пиксель бесконечно далеко от центра; WPT-эталон — сплошной зелёный квадрат), а прочие отказы шейдера пропускают слой. Тест `radial_gradient_with_infinite_centre_does_not_fail_the_snapshot`; `--screenshot` страницы-теста побайтово совпал с эталоном.
