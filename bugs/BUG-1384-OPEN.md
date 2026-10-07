# BUG-1384 — Зависание на `grid-template-columns` из 10⁵ `repeat()`

**Статус:** OPEN
**Заведён:** 2026-10-07 (P6, при закрытии BUG-1315)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid.js` + каскад — `document.body.style.gridTemplateColumns = <2 МБ>`)

## Симптом

`css/css-grid/parsing/grid-template-columns-crash.html` (`gridTemplateColumns` = 100000 раз `repeat(1000, Npx)`, строка ≈ 2,2 МБ) не отвечает по BiDi 20 с (`LUMEN_WPT_HARD_CAP`) → TIMEOUT вместо PASS. Одинаково на двух сборках (с изменениями BUG-1315 и без), значит не от них. Присваивание в шиме занимает ≈ 0,25 с (замер во временном V8-тесте), валидатор — ≈ 0,08 с; тяжёл, судя по всему, пересчёт стиля/раскладки `body` с `grid-template-columns` на 10⁸ дорожек (`repeat(1000, …)` × 10⁵). Найден P6 при закрытии BUG-1315, 2026-10-07.

## Как найдено

`run_report.py --all --root css/css-grid/parsing --recursive --check`: ожидания PASS не выполняются. Те же результаты на бинаре `p2-work/target/dev-release/lumen.exe` (до правки CSSOM) — повторено через `run_smoke.py` на каждом из файлов.

## Что делать

Не исследовано: сначала bisect по `git log` — когда ожидание PASS перестало выполняться, затем починка.

## Как проверить

`css/css-grid/parsing/` — соответствующие файлы, `run_report.py --check`.
