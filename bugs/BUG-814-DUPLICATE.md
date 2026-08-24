# BUG-814 — в глобальной области воркера нет ни `navigator`, ни `location`: чтение любого из них бросает и молча убивает воркер

**Статус:** DUPLICATE → [BUG-776](BUG-776-FIXED.md)
**Слит:** 2026-08-22 — тот же дефект (`worker_global_shim`/`SHARED_WORKER_GLOBAL_SHIM` не заводят `location`/`navigator`), то же направление починки. BUG-776 заведён раньше (2026-08-18, WPT-VENDOR-workers), шире по охвату (плюс сервис-воркер, где `navigator` тоже нет) и потому выживает; **чинить и закрывать нужно его**. Уникальное из этого файла перенесено в BUG-776: живая проба `verify_csp_url_worker_gaps.py` (`--variant worker-navigator`/`worker-async-postmessage`) и корпусной счёт механизма `worker-navigator-missing` (6 id остатка WPT-RUN-5). Файл оставлен целиком: на него ссылаются `timeout_audit.py`-отчёты и заметки среза 18.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 18 — 6 TIMEOUT остатка, механизм `worker-navigator-missing`)
**Область:** `crates/js/src/worker.rs:270-397` (`worker_global_shim` — полный список глобалей воркера), `crates/js/src/shared_worker.rs` (то же для `SharedWorker`: `navigator` не упоминается ни разу)
**Владелец:** P1/P3 (движок, `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Скрипт воркера читает `navigator` — и воркер замолкает навсегда, ещё до
своего первого `postMessage`:

```js
// workers/support/WorkerNavigator.js, сокращённо
postMessage({ appName: navigator.appName, platform: navigator.platform });
//            ^ ReferenceError: navigator is not defined
```

Страница ждёт `worker.onmessage` и не получает ни сообщения, ни ошибки:
исключение внутри воркера наружу не выходит
([BUG-813](BUG-813-FIXED.md)), поэтому симптом — идеальная тишина.

## Прямое измерение

`tests/wpt/verify_csp_url_worker_gaps.py --variant worker-navigator`
(2026-08-21, коммит `41ee56b73`), скрипт воркера постит `typeof` каждого
имени — единственное чтение, которое само не бросает:

```
worker-message data=typeof navigator=undefined self=object
                    location=undefined setTimeout=function
```

Плюс `--variant worker-async-postmessage`: воркер в форме
`(async () => { postMessage(navigator.platform) })()` — как в
`workers/support/WorkerNavigator.js` — не печатает **ничего**, тогда как
echo-воркер в том же прогоне отвечает нормально.

## Причина (локализована чтением кода)

`worker_global_shim` (`worker.rs:270-397`) определяет ровно: `self`,
`name`, `postMessage`, `onmessage`, `addEventListener`,
`removeEventListener`, `_lumen_worker_dispatch_message`, `console`,
`importScripts`, `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`,
`queueMicrotask`, `_lumen_flush_timers`. Отдельно доставляются `atob`/`btoa`,
`EventTarget` и `performance`. `navigator` и `location` не заводит никто —
`grep -n navigator crates/js/src/worker.rs crates/js/src/shared_worker.rs`
пуст.

По HTML LS §10.2.1 `WorkerGlobalScope` обязан иметь `navigator`
(`WorkerNavigator`) и `location` (`WorkerLocation`). Отсутствие `location`
дороже, чем кажется: относительные URL внутри воркера разрешать не от чего.

## Масштаб

Механизм `worker-navigator-missing` забирает **6 id** остатка снимка
WPT-RUN-5 — всё семейство `workers/WorkerNavigator_*` (`appName`,
`appVersion`, `onLine`, `platform`, `userAgent`, `userAgentData`).
Цифра мала по той же причине, что и у [BUG-813](BUG-813-FIXED.md): каталог
`workers/` в снимке в основном отработал по `worker-importscripts`
([BUG-778](BUG-778-FIXED.md)) — до чтения `navigator` тесты не доходят.
Это оценка снизу и одновременно самый дешёвый в починке пункт среза:
объект чисто информационный, вычислять нечего.

## Направление починки (не предписание)

Завести `navigator` в шиме воркера теми же значениями, что отдаёт
`navigator` страницы (`userAgent`, `appName`, `appVersion`, `platform`,
`language`/`languages`, `onLine`, `hardwareConcurrency`) — источник строк
уже один, `CARGO_PKG_VERSION` (см. политику версий в `CLAUDE.md`).
`location` (`WorkerLocation`) — тем же заходом от URL скрипта воркера;
это же даёт базу для разрешения относительных URL внутри воркера. Общий
шим страницы переиспользовать только в части, не трогающей `window`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
   --variant worker-navigator` печатает `typeof navigator=object
   location=object`.
2. `--variant worker-async-postmessage` печатает `async:<platform>,true`.
3. WPT: `run_report.py --all --root workers --recursive` — семейство
   `WorkerNavigator_*` перестаёт висеть.
