# WPT vendor notes — `contacts`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-07-25 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-contacts`, `docs/wpt-status.md`), included despite its 🚫 scope note (Contact Picker API) by the same standing user decision as `accelerometer`/`acid`/`ai`/…/`console`. Same pinned commit, `git sparse-checkout` at the same commit hash, 5 files (`META.yml`, `WEB_FEATURES.yml`, `contacts-select.https.window.js`, `resources/` — `helpers.js`+`non-main-frame-select.html`). No out-of-category helpers. The category's sole test is `.https.`-only (1 selected id) — `run_report.py --all --root contacts --recursive` completed fully: TIMEOUT (`invalid url: invalid port: "None"`, same HTTPS-port gap as `WebCryptoAPI`/`ai`/`compute-pressure`). 0/1 harness OK, 0/0 subtests. Not filed as a separate BUG-NNN — first pass only vendors + runs. See `docs/wpt-status.md` for details.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-25 (`tests/wpt/contacts/`, 5 файлов: `META.yml`, `WEB_FEATURES.yml`, `contacts-select.https.window.js`, `resources/` — `helpers.js`+`non-main-frame-select.html`), включена несмотря на скоуп 🚫 (Contact Picker API — нишевый мобильный API) по прямому запросу пользователя, той же постоянной договорённости, что `accelerometer`/`acid`/`ai`/…/`connection-allowlist`. `run_report.py --all --root contacts --recursive` прошёл полностью (1 отобранный id): единственный `.https.window.js`-тест — TIMEOUT (`invalid url: invalid port: "None"` — минимальный исполнитель не поднимает HTTPS-порт, тот же класс ограничения, что `WebCryptoAPI`/`ai`/`ambient-light`/`compute-pressure`). 0/1 harness OK, 0/0 сабтестов. Не заводился отдельный BUG-NNN — первый проход, см. методологию выше
