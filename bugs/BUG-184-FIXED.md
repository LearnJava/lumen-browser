# BUG-184

**Статус:** FIXED (DEBTOR) 2026-07-04 (KNOWN_DEBTOR, baseline 0.60%)
**Компонент:** paint
**Тест:** TEST-31 (diff 0.59%, KNOWN_DEBTOR)

## Описание

`clip-path`: inset/circle/ellipse/polygon/path bounding-box clip.

## Расследование (2026-06-24, P3)

Геометрия всех clip-форм **верна**. Прогон `--only 31` даёт 0.60% (порог 0.5%).
Декомпозиция diff по рядам теста (порог цвета 16):

| Ряд | Форма | diff-пикселей (femtovg) |
|---|---|---|
| 1 | `inset()` / rect-клипы | **0** (пиксель-в-пиксель) |
| 2 | `circle()` | 1244 |
| 3 | `ellipse()` | 856 |
| 4 | `polygon()` + `path()` | 2108 (доминируют) |
| 5 | combo `inset()` | 110 (edge-шум) |

**Корень:** прямоугольные клипы (`inset()`) идут через `PushClipRect` → femtovg-ножницы
(scissor) → совпадают с Edge идеально. Непрямоугольные формы (circle/ellipse/polygon/path)
рендерятся через `composite_clip_path_layer` — заливкой формы offscreen-слоем как paint
(`fill_path` + `with_anti_alias(true)`). femtovg AA-fringe надувает заливаемую кромку
**~на 1px наружу** относительно AA-ядра Edge. Замер первого круга (`circle(40px)`, центр
device (100,240)): Edge bbox 61–140 / 201–280, Lumen 60–141 / 200–281 — оба центрированы
на 100.5/240.5, но Lumen на 1px шире радиусом по всем 4 сторонам. То же на эллипсе и на
диагональных кромках polygon/path.

## Подтверждение свежей сборкой + CPU-путём (2026-07-04, P3-ревизия)

Свежий full-build `dev-release` + gdigrab:

- **femtovg (окно / gdigrab): 0.59%** (bad=4318), декомпозиция по рядам как выше
  (inset=0, circle 1244, ellipse 856, polygon/path 2108, combo 110) — стабильно, ≈ baseline 0.60%.
- **CPU-путь (`--screenshot`, tiny_skia): 0.06%** (bad=459) — почти пиксель-в-пиксель с Edge.

Тот же display-list, та же геометрия — отличается **только растеризатор**. CPU-снимок
через `rasterize_clip_shape_coverage` (tiny_skia) режет clip-форму с AA-ядром, совпадающим
с Edge (polygon/path 2108→**89**, circle 1244→**188**, ellipse 856→**182**, inset/combo = 0).
Это **опровергает** гипотезу о дефекте геометрии/движка и доказывает: остаток TEST-31 —
исключительно femtovg shape-fill AA-fringe на непрямоугольных clip-масках.

Тот же класс, что **BUG-176** (border-radius edge-AA: CPU 0.208% vs femtovg 0.96%),
**BUG-247** (SVG circle/ellipse curve-AA) и **BUG-173** (SVG stroke AA-швы).

## Почему не чинится до <0.5% (в femtovg-пути)

- Чтобы клип был «туже» (как Edge), нужен **inward-AA** для clip-маски — femtovg
  заливает путь с fringe наружу, внутрь не умеет (нет API покрытия-маски).
- Альтернатива — подгон под точное AA-ядро растеризатора Edge (та же недостижимость,
  что у text-AA, rule 3).
- Доминирующий вклад (2108px) — диагонали polygon/path, которые нельзя инсетнуть на 0.5px
  чисто (требует polygon-offsetting, ломает геометрию).
- Радикальный фикс (растеризовать coverage-маску CPU-side, как tiny_skia) = крупный
  переписыв femtovg-clip-пути, высокий риск, вне scope P3-баг-фикса. CPU-снимок уже
  рисует правильно; окно упирается в femtovg.

## Решение

KNOWN_DEBTOR `'31' → ('BUG-184', 0.60)` в `run.py`. Храповик ±2%: регресс если >2.60%,
снять запись если ≤0.5%. Закроется вместе с общим femtovg edge-AA паритетом (класс
BUG-176/247/173) либо переводом clip-композита на CPU-coverage-маску.
