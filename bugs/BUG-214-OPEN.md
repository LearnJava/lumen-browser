# BUG-214

**Статус:** OPEN (DEBTOR)
**Компонент:** paint
**Тест:** TEST-110 (diff 1.70%)

## Описание

CSS UI `accent-color` — тинт чекбокса/радио/range/progress.

> **Ревизия 2026-06-23 (дрейф трекера):** **реализован** — `emit_form_control_accents`/
> `emit_range_slider` тинтит checkbox/radio/range/progress + юнит-тесты
> (`checkbox_accent_color_tints_indicator`). Diff-картинка TEST-110 подтверждает: цвета-акценты
> применяются верно (cyan/magenta/green), остаток 1.70% = расхождение нативной отрисовки
> form-виджетов (трек/thumb слайдера, стиль progress-бара) vs UA-виджеты Edge — присущее
> кросс-браузерное расхождение (сам тест-HTML отмечает «native control sizes kept»).
> Внесён в KNOWN_DEBTORS (1.70%).

## Воспроизведение

`python graphic_tests/run.py --only 110` → DEBTOR 1.70%
