# WPT vendor notes — `webmessaging`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-webmessaging`, `docs/wpt-status.md`), scope ⬜ (candidate — `window.postMessage`, `MessageChannel`/`MessagePort`, `BroadcastChannel`, all partially implemented in Lumen). Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit hash, `LICENSE-WPT.md` copied from the sibling `webmcp` category, 171 files.

Predictor sweep before vendoring: 0 `name="variant"` hits, 0 `testdriver.js` references, only 3 `.https.` files out of 171 — cheapest-tier category by the established predictors.

Also pulled in 7 out-of-category dependencies discovered via `grep -rhoE '(src|href)=...\.js'` over the category — all were missing from the repo and 404ing on the first run, silently downgrading real per-test signal to infra noise (the same class already documented for `fetch`/BUG-346): `common/get-host-info.sub.js` (+`.headers`), `common/gc.js`, `common/utils.js`, `common/dispatcher/dispatcher.js`, `html/anonymous-iframe/resources/common.js`, `html/cross-origin-embedder-policy/credentialless/resources/common.js`, `service-workers/service-worker/resources/test-helpers.sub.js`. `html/anonymous-iframe/` and `service-workers/service-worker/resources/` were already present in full from the earlier `WPT-VENDOR-html` category vendoring — only `tests/wpt/common/` needed new files.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webmessaging/`, 171 файл, `LICENSE-WPT.md` скопирован из `webmcp`). Скоуп ⬜ — `window.postMessage`/`MessageChannel`/`MessagePort`/`BroadcastChannel` реализованы частично.

`run_report.py --all --root webmessaging --recursive` (~11:30, 136 отобранных id, после довендоривания 7 внекатегорийных зависимостей): **77/136 harness OK, 82/206 сабтестов**. Первый прогон (до довендоривания) дал похожие цифры (79/136, 83/210) — довендоривание убрало 19 ложных 404-провалов, но большинство затронутых тестов упирается в те же корни, что и остальная категория (см. ниже), поэтому итоговый счёт почти не сдвинулся.

Основная масса провалов — уже задокументированные корни:
- TLS `UnknownIssuer` на `.https.`-тестах ([BUG-657](../../bugs/BUG-657-OPEN.md));
- отсутствие отдельного browsing context у `<iframe>`/`window.open()` ([BUG-480](../../bugs/BUG-480-OPEN.md)/[BUG-359](../../bugs/BUG-359-FIXED.md)) — доминирует в `with-ports/`/`without-ports/` численных сериях и `postMessage_crosssite.sub.htm`.

Новый, ранее не описанный сигнал — сам `postMessage`:

- [BUG-717](../../bugs/BUG-717-OPEN.md): `window.postMessage` передаёт сообщение по ссылке вместо структурного клонирования (`new MessageEvent(message)` без `structuredClone`), никогда не валидирует `targetOrigin` как абсолютный URL (молча не доставляет вместо `SyntaxError`), не поддерживает двухаргументную `WindowPostMessageOptions`-форму. `MessagePort.postMessage` в том же файле делает клонирование правильно — асимметрия одного файла.
- [BUG-718](../../bugs/BUG-718-OPEN.md): `BroadcastChannel.postMessage` клонирует через `JSON.stringify` вместо `structuredClone` — не бросает на 0 аргументов, не бросает `DataCloneError` на `Symbol()` (превращает в `'null'` вместо распознавания как неклонируемое), теряет типы (`Map`/`Set`/`Date`/typed arrays), циклы дают обычный `TypeError` вместо `DataCloneError`.

Оба находятся живьём в исполнившихся тестах (не TLS/testdriver-заглушка) — `broadcastchannel/interface.any.html` (10/13) и три файла `with-options/*.html` (`resolving broken url`/`resolving 'example.org'`/`without-ports`/`without-ports/014.html`-паттерн "structured clone vs reference").
