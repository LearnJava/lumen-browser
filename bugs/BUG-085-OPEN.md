# BUG-085

**Статус:** OPEN (частично исправлен — repeating + hard-stop, 12.05% → 1.62%)
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

linear/radial gradient TEST-39 расходился с Edge на 12.05%.

## Исправлено 2026-06-19 — femtovg_stops (repeating + hard-stop tail-fill)

Корень: femtovg-бэкенд (`DrawLinearGradient`/`DrawRadialGradient`) **игнорировал
флаг `repeating`** и просто clamp-ил позиции стопов в [0,1]. femtovg синтезирует
256-тексельную текстуру градиента (`gradient_store.rs`), заполняя её от 0 до
первого стопа и между соседними стопами, но **область за последним стопом
оставляет прозрачной** и игнорирует позиции вне [0,1]. Поэтому:

1. `repeating-linear-gradient` / `repeating-radial-gradient` (TEST-39 rows 2-3)
   рисовали один clamp-период вместо повторения.
2. Hard-stop без завершающего стопа (`… green 50%`) рисовал только первую
   половину, вторая оставалась прозрачной (тот же дефект в TEST-30 row 2).

Фикс: новый `femtovg_stops(stops, line_len, repeating)`:
- **non-repeating:** продлевает последний цвет до 1.0, если последний стоп < 1.0
  (хвост дозаполняется);
- **repeating:** замощает паттерн (период = `last − first`) по всей линии [0,1],
  позиции остаются монотонными (граница повтора = совпадающие позиции).

Попутно `linear_gradient_line_len(w, h, angle)` даёт корректную длину CSS-линии
для непрямых углов (45°) — px-стопы делятся на неё, а не на `rect.width`.

Проверено: TEST-39 12.05% → **1.62%**; row 1 (простые linear) совпадает с Edge
пиксель-в-пиксель (вне diff-региона). TEST-30 (BUG-144) 10.48% → **7.56%**
(row 2 hard-stop). 4 unit-теста (`femtovg_stops_*`).

## Остаток (DEBTOR, 1.62%, KNOWN_DEBTORS '39')

1. **256-тексельная квантизация** градиент-текстуры femtovg: repeating-границы
   ложатся на гранулярность текселя (±1 текс ≈ ±0.7px), накапливается по многим
   границам (rows 2-3). Устранимо только обходом femtovg-градиента (CPU-заливка),
   что не оправдано на 1.6%.
2. **Radial-интерполяция/AA** vs Edge downscale-kernel.
3. gdigrab суб-пиксельный шум.
