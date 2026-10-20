# BUG-1383 — Регресс в `main`: `getComputedStyle().gridTemplateColumns` пуст

**Статус:** FIXED 2026-10-20 (P3)
**Исправление:** симптом (`""` вместо значения) устранён BUG-1307 (`grid_computed::insert_grid_computed`, used-размеры из раскладки): оба файла теперь завершаются `OK`, а не `TIMEOUT`. Проверено `run_smoke.py` на dev-release: `grid-columns-rows-get-set-multiple` 26/40, `grid-content-sized-columns-resolution` 0/42 — оставшиеся падения не про пустую строку. Остаток: [BUG-1656](BUG-1656-OPEN.md) (невалидный `none auto`, `inherit`), [BUG-1657](BUG-1657-OPEN.md) (размеры auto/content-sized дорожек). `.ini`-ожидания `TIMEOUT` (BUG-467) устарели, их обновление — после BUG-1657.
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
