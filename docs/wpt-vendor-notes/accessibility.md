# WPT vendor notes — `accessibility`

## Vendoring (`tests/wpt/VENDOR.md`)

Sixth full test category, added 2026-07-23 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-accessibility`, `docs/wpt-status.md`). Same pinned commit, `git sparse-checkout` at the same commit hash, 59 files (`ReadMe.md`, `crashtests/`, one testdriver.js test). No out-of-category helpers. Almost the whole category (58/59 files) is WPT-manifest type `crashtest`, which the minimal `browsers/lumen.py` product plugin has no executor for at all (`Unsupported test type crashtest for product lumen` — excluded before the run queue, not a per-test failure); see `docs/wpt-status.md` for the run results.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-23 (коммит `344c7afb`, `tests/wpt/accessibility/`, 59 файлов; ReadMe.md + `crashtests/` + один testdriver.js-тест). Внекатегорийных хелперов не обнаружено. Категория почти целиком (58/59) — не-testharness `crashtests/`: манифест WPT классифицирует их как тип `crashtest`, для которого у минимального исполнителя `browsers/lumen.py` вовсе нет реализации (`Unsupported test type crashtest for product lumen`) — они даже не попадают в очередь прогона, а не проваливаются как тест. Прогон `run_report.py --all --root accessibility --recursive`: выбран 1 реально исполнимый id (`svg-mouse-listener.html`, тип `testharness`), результат — `SKIP` (`Executor does not support testdriver.js`, тот же класс ограничения, что в `accelerometer`/`IndexedDB`). Не заводился отдельный BUG-NNN — первый проход, см. методологию выше; для полноценного покрытия этой категории потребовалась бы отдельная реализация `crashtest`-executor'а (проверка "страница загрузилась и не крашнула браузер" без `testharness.js`), это отдельная задача инфраструктуры, не движковый баг
