# WPT vendor notes — `parakeet`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/parakeet/`, 3 файла: `META.yml`, `createAdRequest.tentative.https.sub.window.js`, `finalizeAd.tentative.https.sub.window.js`, `idlharness.tentative.https.window.js`; `LICENSE-WPT.md` скопирован из соседней `orientation-sensor`). Ни variant-ов; все 3 файла `.https.`, 2/3 тянут `testdriver.js`/`testdriver-vendor.js`. `run_report.py --all --root parakeet --recursive` (~1 мин, 3 id): **0/3 harness OK, 0/0 сабтестов**. Все три TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer` — ни один тест не исполнился. Живая проба (`--dump-layout` + инлайн-скрипт, `typeof navigator.createAdRequest/finalizeAd/runAdAuction/joinAdInterestGroup/deprecatedURNToURL`) подтвердила: все пять — `undefined`, API (Microsoft PARAKEET / Privacy Sandbox ad-tech) не реализован вовсе, что соответствует скоупу 🚫. Новых багов не заведено
