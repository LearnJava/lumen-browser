# WPT vendor notes — `accname`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-07-23 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-accname`, `docs/wpt-status.md`, commit `69ab520d`). Same pinned commit, `git sparse-checkout` at the same commit hash, 183 files. Entirely `testdriver.js`-dependent — the minimal executor has no `test_driver.*` support, so all 19 selected ids come back `SKIP`; see `docs/wpt-status.md` for the run results.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-23 (коммит `69ab520d`, `tests/wpt/accname/`, 183 файла: `manual/`-подкаталог — тесты для ручной проверки, не рассчитаны на автоматизацию через testharness.js; `name/` — `comp_*.html`, вычисление accessible name/description; корневые `aria-owns.html`/`basic.html`). Внекатегорийных хелперов не обнаружено. Прогон `run_report.py --all --root accname --recursive`: 19 отобранных id (`manual/`-подкаталог не попал в выборку — не `testharness`), все 19 — `SKIP` (`Executor does not support testdriver.js`, тот же класс ограничения, что в `accelerometer`/`accessibility`/`IndexedDB`), 0/19 harness OK, 0/0 сабтестов. Категория целиком зависит от `testdriver.js` (симуляция пользовательских действий/фокуса при проверке accessible name) — без отдельного testdriver-исполнителя недостижима для автоматизации. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше
