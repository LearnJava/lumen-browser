# WPT vendor notes — `viewport-segments`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (`tests/wpt/viewport-segments/`, 3 файла: `viewport-segments-change-event.https.html`, `viewport-segments-env-variables.https.html`, `viewport-segments-segments-property.https.html`), включена несмотря на скоуп 🚫 (складные устройства). Прогон `run_report.py --all --root viewport-segments --recursive`: 3 отобранных id, все `.https.` — **0/3 harness OK**, каждый упирается в уже задокументированный TLS-гэп (`UnknownIssuer`, self-signed сертификат wptserve не в доверенном хранилище Lumen). Живая проба (`--mcp-live-port`, `JSON.stringify({hasVisualViewport: typeof window.visualViewport, ...})`) подтвердила: `window.visualViewport` отсутствует целиком (не `undefined`-проперти на объекте — самого объекта нет), не только `.segments` — реконфирмация уже открытого [BUG-481](../bugs/BUG-481-OPEN.md). Новый номер бага не заводился
