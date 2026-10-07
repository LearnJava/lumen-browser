# BUG-1383 — Регресс в `main`: `getComputedStyle().gridTemplateColumns` пуст

**Статус:** OPEN
**Заведён:** 2026-10-07 (P6, при закрытии BUG-1315)
**Область:** js/layout (`getComputedStyle(el).gridTemplateColumns` — Rust-сторона `_lumen_get_computed_style`)

## Симптом

`getComputedStyle(el).gridTemplateColumns` / `gridTemplateRows` отдаёт `""` вместо `none` (значение по умолчанию) и вместо разрешённых размеров дорожек (`45px 45px`). В `tests/wpt/metadata` стоит ожидание PASS на 82 подтеста `css/css-grid/parsing/grid-columns-rows-get-set-multiple.html` (40) и `grid-content-sized-columns-resolution.html` (42) — сейчас все FAIL. Воспроизводится на двух независимых сборках `lumen` (p2-work `ae75de0e1` и p6-work без правок CSSOM), то есть регресс уже в `main`. Найден P6 при закрытии BUG-1315, 2026-10-07.

## Как найдено

`run_report.py --all --root css/css-grid/parsing --recursive --check`: ожидания PASS не выполняются. Те же результаты на бинаре `p2-work/target/dev-release/lumen.exe` (до правки CSSOM) — повторено через `run_smoke.py` на каждом из файлов.

## Что делать

Не исследовано: сначала bisect по `git log` — когда ожидание PASS перестало выполняться, затем починка.

## Как проверить

`css/css-grid/parsing/` — соответствующие файлы, `run_report.py --check`.
