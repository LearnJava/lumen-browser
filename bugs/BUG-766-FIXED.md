# BUG-766 — `isSecureContext` отсутствует в `WorkerGlobalScope`

**Статус:** FIXED 2026-09-24 (P1, WORKER-1)
**Компонент:** js (`crates/js/src/shim/worker_location_navigator_shim.js` —
`_lumen_worker_secure_context_for`; `crates/js/src/worker.rs`'s
`worker_global_shim`, `crates/js/src/shared_worker.rs`'s
`SHARED_WORKER_GLOBAL_SHIM`, `crates/js/src/sw_worker.rs`'s
`sw_globals_shim` — все три ставят `globalThis.isSecureContext`)
**Найден:** P3, при закрытии [BUG-399](BUG-399-FIXED.md), 2026-08-11

## Симптом

`worker_global_shim` (глобал воркер-потока) не заводил `isSecureContext`
вовсе: `'isSecureContext' in self === false`, чтение давало `undefined`.
Свойства не было ни в каком виде — это не «всегда `true`», как было в окне
до [BUG-399](BUG-399-FIXED.md), а дыра в поверхности.

По HTML LS `isSecureContext` объявлен в миксине `WindowOrWorkerGlobalScope`
(`[Exposed=(Window,Worker)]`), то есть обязателен и в воркере.

## Причина

`worker_global_shim` — сокращённый стаб: он заводил `self`/`name`/
`postMessage`/`addEventListener`/`console`/`importScripts`/таймеры и на этом
заканчивался. Ни `location`, ни `navigator`, ни `performance`
([BUG-401](BUG-401-FIXED.md)), ни `isSecureContext` в нём не было.

## Исправление

Все три вида воркера (dedicated, shared, service) уже строят свой
`location` из собственного URL через общий `_lumen_make_worker_location`
(`worker_location_navigator_shim.js`, часть `WORKER_LOCATION_NAVIGATOR_SHIM`,
BUG-776). Рядом с ним заведена вторая функция того же файла,
`_lumen_worker_secure_context_for(url)` — самостоятельная копия
трастворти-правила Secure Contexts §3.1/§3.2 (`about:`/`data:`/`blob:`
короткое замыкание, `https:`/`wss:`/`file:`, loopback-хост IPv4/IPv6),
которое страница считает через `_lumen_url_is_potentially_trustworthy`
(`web_api_shim_mid_b.js`). Копия, а не общий кусок: страничная функция живёт
в page-only шиме (замыкание над `document`/`window`-зависимым контекстом
отсутствует, но сам файл эволюционно вырезан как часть страничной сборки,
не воркерной), и та же логика уже применена к сиблингам
(`WORKER_ERROR_EVENT_SHIM`/`WORKER_MESSAGE_EVENT_SHIM`) — расхождение двух
копий потребовало бы намеренной правки обеих сразу.

Каждый из трёх воркерных глобал-шимов сразу после установки `location`
вызывает `_lumen_worker_secure_context_for` с тем же URL (dedicated/shared —
`_lumen_worker_location_url`, тот же Rust-глобал, что строит `location`;
service — `globalThis.location.href`, поскольку у него `location` строится
из `scope`/`origin`, а не из одного готового URL) и ставит результат
неперезаписываемым (`configurable: true`, без сеттера) `isSecureContext`.

Отклонение от постановки: вместо протаскивания уже посчитанного на главном
потоке значения (что требовало бы нового параметра через все три
`spawn_*_v8`) значение считается заново из собственного URL воркера —
тот же URL, из которого уже строится `location`, и та же сеть/CSP-проверка,
что фетчила скрипт воркера, уже применила тот доверительный барьер, который
спека описывает как «наследуется от environment settings object создателя».

## Гейт

Юнит-тесты на все три вида воркера (`v8_worker_has_is_secure_context_true_on_https`,
`v8_worker_has_is_secure_context_false_on_insecure_origin`,
`v8_worker_has_is_secure_context_true_on_loopback_http` в `worker.rs`;
`shared_worker_global_scope_has_is_secure_context` в `shared_worker.rs`;
`sw_global_scope_has_is_secure_context` в `sw_worker.rs`) — https, plain
http (не loopback) и loopback http. `cargo test -p lumen-js --features
v8-backend --lib` — 4226 passed, 1 failed (предсуществующий флак
`frame_bridge::inaccessible_bridge_mutation_does_not_mark_dirty`, BUG-1110,
не связан). `cargo clippy -p lumen-js --all-targets --features v8-backend
-- -D warnings` и `cargo clippy --workspace --all-targets -- -D warnings` —
чисто. JS-only правка, `display-list`/пиксельный гейт не требуется.

## Связанные

* [BUG-399](BUG-399-FIXED.md) — окно; источник трастворти-правила, которое
  этот баг переносит в воркер.
* [BUG-401](BUG-401-FIXED.md) — `performance` отсутствовал в том же
  глобале; тот же класс «сокращённый стаб воркера».
* [BUG-765](BUG-765-FIXED.md) — гейт `[SecureContext]` в воркере был нечем
  питать до этого фикса.
