# WPT vendor notes — `webstorage`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webstorage`, `docs/wpt-status.md`), scope ⬜ (in scope). Confirmed
before vendoring: `localStorage`/`sessionStorage` are a real, persisted
implementation (`crates/js/src/v8_runtime.rs:3519-3569`, native
`_lumen_ls_*`/`_lumen_ss_*` bindings backing a real per-origin store), not a
stub — `StorageEvent` is also wired (`crates/js/src/dom.rs:625-640`).

Same pinned upstream commit `35be3b44`, `git sparse-checkout add webstorage`
at that commit, `LICENSE-WPT.md` copied from a sibling category — 86 files
(85 upstream + license), 54 glob ids. Cheap predictors across the board: 0
`name="variant"` hits, 0 `testdriver.js` hits, 1 `.https.` file. Confirmed
cheap in practice: ~7 min 40 s wall-clock, single process.

### Harness prerequisite inherited from `websockets` (previous category)

The previous session (`WPT-VENDOR-websockets`, same day) permanently
enabled the `ws`/`wss`/`h2` ports in `tests/wpt/config.json` (previously
`null`, which made `tools/serve/serve.py::start_servers` skip those server
schemes entirely). Since `wptserve` starts **every configured scheme
unconditionally** regardless of whether the running category needs it, this
means every vendored category's run now depends on `wss` actually starting
— not just protocol-testing categories. The first `webstorage` run attempt
failed immediately with `OSError: Servers failed to start: wss:18889` /
`module 'ssl' has no attribute 'wrap_socket'` (Python 3.12+ removed
`ssl.wrap_socket`, and `websockets`'s local, uncommitted `.venv` patch —
documented in `docs/wpt-vendor-notes/websockets.md` — was not present in
this session's `tests/wpt/.venv`, likely a different venv instance/re-install
since `.venv` is `.gitignore`d wholesale). Re-applied the same class of fix
locally: replaced `pywebsocket3`'s deprecated
`ssl.wrap_socket(sock, keyfile=..., certfile=..., ca_certs=..., cert_reqs=...)`
call (`websocket_server.py:160`) with the modern `ssl.SSLContext`-based
equivalent (`SSLContext(PROTOCOL_TLS_SERVER)` + `load_cert_chain` +
`load_verify_locations` + `wrap_socket(sock, server_side=True)`). Not
committed (site-packages, `.venv/.gitignore` is a bare `*`) — **every future
WPT-VENDOR session on a fresh/reinstalled venv will hit this again** until
`tests/wpt/requirements.txt` pins a `pywebsocket3` version that doesn't call
the removed API, or a small vendored patch is carried outside `.venv`.

### Run result

`run_report.py --all --root webstorage --recursive` (~7 min 40 s,
single process): **24/54 harness OK, 63/1270 subtests passed** — real
signal, most of the harness the run actually executes (only 1 harness
TIMEOUT that isn't part of the two clusters below:
`storage_local_setitem_quotaexceedederr.window.js`, not investigated this
session).

Two confirmed, filed root causes account for the large majority of
subtest failures:

- **[BUG-773](../../bugs/BUG-773-OPEN.md)** — `localStorage`/`sessionStorage`
  are plain JS objects (`_lumen_make_storage`, `dom.rs:9055`), not a
  `Proxy`-backed "legacy platform object" per HTML §8. Property-style access
  (`storage.foo = 'x'`), `for-in`/`Object.keys`, and the `in`/`delete`
  operators never reach the native `getItem`/`setItem` backend — they create
  or read ordinary JS instance properties instead, giving the object **two
  disjoint data planes**. Methods are enumerable own-instance properties
  instead of members of a shared, non-enumerable `Storage.prototype`, and
  `window.Storage` (the interface object tests reference directly, e.g.
  `symbol-props.window.js`) doesn't exist at all. Directly explains
  `storage_enumerate.window.js`, `storage_length.window.js` ("method
  access" subtest), `storage_string_conversion.window.js`, the `"in
  storage"` assertions in `storage_removeitem.window.js`, and all of
  `symbol-props.window.js`.
- **[BUG-774](../../bugs/BUG-774-OPEN.md)** —
  `StorageEvent.prototype.initStorageEvent` (`dom.rs:636`) assigns its
  eight arguments as-is, with no WebIDL string coercion for `type`/`url`
  and no default-value substitution (`null`) for an omitted/`undefined`
  `key`/`oldValue`/`newValue`. 3 of 5 `event_initstorageevent.window.js`
  subtests fail on exactly this (e.g. `initStorageEvent('type')` alone
  should leave `event.key === null`, actually leaves it `undefined`).

**Everything else traces to the already-filed
[BUG-480](../../bugs/BUG-480-OPEN.md)** (`<iframe>` has no separate
browsing context) — the entire `event_*.html` cluster (10 files, all via
`eventTestHarness.js`'s `document.body.appendChild(iframe); ...
iframe.contentWindow.onstorage = ...`) and `document-domain.html`
(`frames[0].addEventListener(...)`, `frames[0]` undefined) TIMEOUT or FAIL
on `iframe.contentWindow`/`frames[0]` being null/undefined, not on anything
Storage-specific. Not re-filed; confirmed by reading each failing test's
source before attributing it here, per the project's "don't describe a
failure mode from intuition" rule — every claim above traces to a specific
line in a specific vendored `.js` file, not a guess from the log alone.

The remaining ~4 partitioned-storage tests (`*-basic-partitioned.sub.html`,
`localstorage-about-blank-3P-iframe-opens-3P-window.partitioned.html`,
`localstorage-share-data-unrelated-origins.html`) TIMEOUT on Storage
Partitioning (a newer, opt-in HTML spec feature Lumen doesn't implement at
all) — not investigated further, no bug filed (whole-feature absence, same
class as other unimplemented-spec-feature gaps already on record elsewhere
in this project, not a defect in an implemented surface).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён точно перед вендорингом (`localStorage`/
`sessionStorage` — реальный персистентный бэкенд, `v8_runtime.rs:3519-3569`,
не заглушка). Вендорена целиком 2026-08-18 (коммит `35be3b44`,
`tests/wpt/webstorage/`, 86 файлов, 54 id, дёшево по всем предикторам:
0 variant, 0 testdriver, 1 `.https.`).

Первый прогон упал: `wss`-порт (включённый предыдущей сессией
`WPT-VENDOR-websockets` навсегда в `config.json`) не стартовал —
`pywebsocket3` дёргает удалённый в Python 3.12+ `ssl.wrap_socket()`, а
локальный некоммитящийся патч `.venv` той сессии не пережил (другой
venv/переустановка). Патч применён заново (тот же класс фикса —
`ssl.SSLContext`), локально, не в репозитории.

`run_report.py --all --root webstorage --recursive` — ~7 мин 40 с,
**24/54 harness OK, 63/1270 сабтестов**. Два подтверждённых корня
объясняют основную массу провалов: [BUG-773](../bugs/BUG-773-OPEN.md)
(`Storage` — не «legacy platform object» спеки, property-style
доступ/`for-in`/`in`/`delete` идут мимо нативного бэкенда, два несвязанных
слоя данных на одном объекте) и [BUG-774](../bugs/BUG-774-OPEN.md)
(`initStorageEvent` без WebIDL-коэрсии/дефолтов). Остальное — уже
заведённый [BUG-480](../bugs/BUG-480-OPEN.md) (`<iframe>` без browsing
context, весь кластер `event_*.html`/`document-domain.html`) и
непокрытая Storage Partitioning (новая опциональная фича, не заведена —
целиком отсутствующая возможность, не дефект реализованной поверхности).
