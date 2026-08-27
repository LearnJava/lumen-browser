# WPT vendor notes — `web-install`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (пин `35be3b44`, `tests/wpt/web-install/`, 4 тестовых файла + `resources/navigator-install-iframe-helper.html` + `WEB_FEATURES.yml`), включена несмотря на скоуп 🚫 (PWA-инсталляция). `run_report.py --all --root web-install --recursive`: 4 отобранных id (все `.tentative.https.`): **0/4 harness OK**. Все 4 TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer` ([BUG-657](../../bugs/BUG-657-OPEN.md)). Живая проба (`--mcp-live-port`) API категории — `navigator.install` — подтвердила: метод отсутствует целиком (`typeof navigator.install === "undefined"`), ни одного `install`-подобного имени в прототипе `Navigator` или на самом `navigator`, ни `onappinstalled`, ни `onbeforeinstallprompt` на `window` — ожидаемо для 🚫-скоупа, не баг. Новый номер бага не заводился
