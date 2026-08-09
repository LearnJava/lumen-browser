# WPT vendor notes — `FileAPI`

## Vendoring (`tests/wpt/VENDOR.md`)

Second full test category, added 2026-07-21 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-FileAPI`, `docs/wpt-status.md`). Same pinned commit, `git sparse-checkout` at the same commit hash. A handful of its tests reference helper scripts outside `FileAPI/` (`/common/*.js`, `/resources/idlharness.js`, `/html/anonymous-iframe/resources/common.js`, `/service-workers/service-worker/resources/test-helpers.sub.js`) that were **not** vendored — those specific tests fail with a 404 for the missing helper, a documented survey gap in `docs/wpt-status.md`, not a Lumen engine bug.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-21 (коммит `35be3b44`, `tests/wpt/FileAPI/`, 115 файлов; `common/`/`html/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон `run_report.py --all --root FileAPI --recursive`: 66/70 id получили результат (4 — `.https.html`-тесты не добежали), 35/66 harness OK, 115/305 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `Blob.prototype.bytes()`/`.textStream()` отсутствуют; конструктор `Blob`/`File` не поддерживает опцию `endings`; `File-constructor-endings.html` возвращает пустое содержимое (0/34, хуже симметричного Blob-теста); `FileReader.readyState`-трекинг в ряде сабтестов не совпадает с ожиданиями; 4 теста `*.https.html` в `BlobURL/` не добежали (`invalid url: invalid port: "None"` — минимальный исполнитель не поднимает HTTPS-порт, тот же класс ограничения, что и отсутствие iframes/multi-window)
