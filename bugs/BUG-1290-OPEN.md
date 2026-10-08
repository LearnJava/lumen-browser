# BUG-1290 — CPU-растр сплющивает 3D-матрицу `PushTransform` до 2D-аффинной: `perspective`, `translateZ`, `rotateX/Y` рисуются неверно

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs`, `PushTransform` → `matrix_util::mat4_to_2d_affine` отбрасывает z-строку и деление на w)

## Симптом

`--screenshot` 600×200 (`--dump-display-list` печатает только 2D-часть матрицы и маскирует дефект):

| разметка | получено | ожидается |
|---|---|---|
| родитель `perspective:100px`, ребёнок 50×50 `translateZ(50px)` | 50×50, без изменений | 100×100 (масштаб 2 вокруг центра) |
| ребёнок `transform:perspective(100px) translateZ(50px)` | 50×50, матрица сдвига (−125, −25), масштаба нет | 100×100 |
| `transform:perspective(100px) translateZ(25px)` | не нарисован | 67×67 |
| `perspective(100px) rotateY(60deg)`, 50×50 | матрица `[0.933 2.165 0 1 …]`, высота 130 px | трапеция высотой ≈ 50 px |
| `rotateX(45deg)` → вложенный `rotateX(-45deg)` при `preserve-3d`, 100×200 | ≈149×239 | плоский 100×200 |

Тот же `rotate(45deg)`/`scale(2)` без 3D работают. Комментарий у `mat4_to_2d_affine`: «3D-составляющие отбрасываются —
вызывающий код обязан проверить `is_2d_affine`»; в `PushTransform` проверки нет.

## Как найдено

WPT-RUN-14 срез 5: 77 упавших reftest `css/css-transforms` (`transform3d-*`, `perspective-*`, `preserve3d-*`,
`backface-visibility-*`, `3d-*`); wptrunner снимает reftest через CPU-растр (`lumen --ipc-server`).

## Что делать

Для проективной матрицы (`!is_2d_affine()`) — перепроецировать четыре угла (и содержимое слоя) с делением на w:
tiny-skia умеет только аффинные `Transform`, нужен свой обратный выборщик по слою (билинейный) либо разбиение на
треугольники. Живое окно (wgpu) заявлено как перспективно-корректное (`CSS-SPECS.md` `perspective`) — не проверялось
этим срезом; если там верно, то дефект только в CPU-пути (эталонный снимок, графические тесты, WPT).

## Как проверить

`css/css-transforms/transform3d-perspective-006.html` (50×50 в `padding:25px`, ожидается `lime`-квадрат 100×100),
`transform3d-preserve3d-009.html`, `perspective-origin-004.html`; `run_corpus.py --prefixes css/css-transforms`.
