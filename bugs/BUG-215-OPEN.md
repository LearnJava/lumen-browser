# BUG-215

**Статус:** OPEN (DEBTOR)
**Компонент:** layout
**Тест:** TEST-113 (diff 1.41%)

## Описание

CSS Shapes `shape-outside: path()` — float обтекание треугольного SVG-контура.

> **Ревизия 2026-06-23 (дрейф трекера):** **реализован** — `parse_shape_path_px` флэттит
> path()→полигон, текст обтекает диагональ через `FloatContext`
> (`float_context_path_left_float` + `parse_shape_path_*` тесты). Diff-картинка TEST-113
> подтверждает: фича работает, остаток 1.41% = AA вдоль диагональной кромки shape + font-parity
> обтекающего текста (rule 2/3). Внесён в KNOWN_DEBTORS (1.41%).

## Воспроизведение

`python graphic_tests/run.py --only 113` → DEBTOR 1.41%
