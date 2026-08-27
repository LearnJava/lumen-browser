# WPT vendor notes — `websockets`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-websockets`, `docs/wpt-status.md`), scope ⬜ (in scope). Unlike
every `webrtc-*` category vendored so far, Lumen has a real, network-backed
`WebSocket` implementation (`crates/network/src/websocket/mod.rs`, wired
through `crates/js/src/dom.rs`'s `WebSocket` shim and
`lumen_core::ext::JsWebSocketProvider`) — confirmed before vendoring via
`grep -rln "JsWebSocketProvider" crates/*/src/`, which found real impls in
`crates/network/src/lib.rs` (not a stub).

Same pinned upstream commit `35be3b44`, `git sparse-checkout add websockets`
at that commit, `LICENSE-WPT.md` copied from a sibling category — 266 files,
217 glob ids (`run_report.py`'s count). Heavy `name="variant"` fan-out
(`?default` / `?wss` / `?wpt_flags=h2`, ~3x on most files, matching the
`constants.sub.js`-driven scheme-switch pattern), zero `testdriver.js` hits,
2 `.https.` files. Category also carries `handlers/*.py` — real
`pywebsocket3` WebSocket-Handler scripts (`echo_wsh.py`,
`basic_auth_wsh.py`, cookie/backpressure/close variants) served by the
harness's own `ws`/`wss` daemons, not by the plain HTTP/HTTPS servers.

### Two harness-level fixes required (not vendored-file edits)

This is the **first** vendored category whose tests need a raw
`ws://`/`wss://` protocol server, so it's also the first to exercise two
previously-dormant paths in `tests/wpt/config.json`
(this project's own harness override file, not part of the unmodified-vendor
`tools/wptrunner`/`tools/serve` tree — safe to edit, confirmed via
`git log --follow` showing it was authored by earlier P2-wpt sessions,
WPT-RUN-2/`b34f18e4b`):

1. **`ws`/`wss`/`h2` ports were hardcoded `null`.** `tools/serve/serve.py::start_servers`
   skips starting a server scheme entirely when its configured port is
   `None` (`for port in ports: if port is None: continue`) — no prior
   vendored category needed a raw-protocol port, so nobody had turned these
   on. Fixed by setting real port numbers (`ws: 18888`, `wss: 18889`,
   `h2: 19000`) in `tests/wpt/config.json`. `wss` additionally needed a
   **local, uncommitted** `.venv` patch: `pywebsocket3` 4.0.2's
   `websocket_server.py:160` calls the deprecated `ssl.wrap_socket()`, which
   Python 3.12+ removed — the installed venv's Python 3.14 raised
   `module 'ssl' has no attribute 'wrap_socket'` on every `wss` startup
   attempt. Not fixed in the repo (site-packages aren't vendored/committed —
   `tests/wpt/.venv/.gitignore` is a bare `*`); worth flagging for whoever
   next touches `tests/wpt/requirements.txt` — either pin an older
   `pywebsocket3`/Python, or carry a small vendored patch/fork if `wss`
   coverage is to survive a fresh `pip install`.
2. **`ws_doc_root` had no override, and its only non-null default is wrong.**
   Same *class* of gotcha already on record in `CLAUDE.md` for
   `/resources/testdriver.js`: `tools/serve/serve.py`'s `ConfigBuilder`
   defaults `ws_doc_root` to `os.path.join(repo_root, "websockets",
   "handlers")`, where `repo_root` (`tools/localpaths.py`) is computed by
   walking one `os.pardir` up from `tools/`'s own location — which lands on
   the *Lumen* repo root (`tools/` is vendored there, not at a wpt-checkout
   root), giving `<lumen-root>/websockets/handlers` instead of
   `tests/wpt/websockets/handlers`. Unlike `doc_root` (which `environment.py
   ::build_config()` explicitly re-points at `serve_path(test_paths)` in
   code, unconditionally), `ws_doc_root` has no equivalent code-level fix —
   but `_get_ws_doc_root`'s own fallback (`if data["ws_doc_root"] is None:
   return os.path.join(data["doc_root"], "websockets", "handlers")`) is
   exactly the correct, portable computation, *if* the raw value is `None`
   rather than the (wrong) class default. Fixed with one line in
   `tests/wpt/config.json`: `"ws_doc_root": null` — forces the fallback,
   no `tools/wptrunner`/`tools/serve` file touched.

A third apparent failure (`constants.sub.js` still 500ing on `?default`
*after* both fixes) turned out to be a **stale leftover server process**
from an earlier failed attempt still bound to port 18300 with pre-fix
config baked in — not a code bug. Diagnosed by writing a standalone script
that reconstructs `ConfigBuilder` exactly as `environment.py::build_config()`
does and printing `final.ports` (matched the fix), then confirming live via
a temporary debug `print()` in `pipes.py::config_replacement` (reverted
before commit — `git diff tools/` is clean). Lesson for the next raw-port
category: kill any prior `python.exe`/`lumen.exe` from earlier smoke
attempts before trusting a "still 500" reading on a config change.

### Run result

`run_report.py --all --root websockets --recursive` (~22 min wall-clock,
single process): **342/515 harness OK, 166/934 subtests passed** — a real
result, not a wall of SKIP/TIMEOUT. `?default` (plain `ws://`) tests
genuinely execute against Lumen's real `WebSocket` implementation, several
passing outright (`Test OK, Subtests passed 1/1`). `?wss`/`?wpt_flags=h2`
variants (h2 also routes through `wss://` per `constants.sub.js`) uniformly
fail on the already-documented TLS gap (`docs/wpt-status.md:20-28`,
`tests/wpt/certs/README.md` — self-signed test cert, `UnknownIssuer`), the
same class every other `.https.`-heavy category hits; not a new finding.

**132 harness ERRORs** are all the already-documented `BUG-438`-adjacent
`browsingContext.navigate(...) reported success but the document was never
replaced` cascade on the `?wpt_flags=h2` variant's failed TLS handshake —
confirmed by grepping every `TEST_END: ERROR` line for that exact phrase
(132/132 matched, zero other ERROR causes).

**17 harness TIMEOUTs.** Three (`Create-blocked-port.any.html`, all three
variants) trace to one clear, filed, high-confidence engine gap:
[BUG-772](../../bugs/BUG-772-FIXED.md) — `WebSocket` has no port-blocking
check (WHATWG Fetch §3.9's 92-port blocklist), so the test's 92
`CreateWebSocketWithBlockedPort(N)` calls each attempt a real TCP connect
(~2.4–2.9 s per refused port on this machine) instead of a synchronous
`SecurityError`, adding up to far more than wptrunner's external per-test
timeout. `grep -rn "blocked_port\|bad_port\|BLOCKED_PORTS" crates/network/src/`
confirms zero existing implementation — not WebSocket-specific either;
`fetch()`/XHR were not re-checked this session.

The remaining ~14 subtest-level TIMEOUTs are spread across
`cookies/003.html` (`HttpOnly` cookie on the WS handshake), `interfaces/
WebSocket/close/close-connecting.html` (`close()` while still `CONNECTING`),
`interfaces/WebSocket/send/{005,010}.html`, `send-many-64K-messages-with-
backpressure.any.js` (`bufferedAmount` backpressure), `remove-own-iframe-
during-onerror.window.html` (likely the same missing-iframe-browsing-context
gap as BUG-480, not confirmed), `unload-a-document/{003,004}.html`, and
`Create-on-worker-shutdown.any.js` — each a plausible distinct gap, **none
root-caused this session** (time budget went to the two harness fixes above
and BUG-772); worth a follow-up pass before/instead of moving to the next
WPT-VENDOR category if P2 revisits protocol-level categories.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён точно перед вендорингом (`WebSocket` — реальная
сетевая реализация, `crates/network/src/websocket/mod.rs`, не заглушка,
в отличие от всего семейства `webrtc-*`). Вендорена целиком 2026-08-18
(коммит `35be3b44`, `tests/wpt/websockets/`, 266 файлов, 217 id, тяжёлый
variant-фан-аут `?default`/`?wss`/`?wpt_flags=h2`, testdriver — 0 хитов).

Потребовались два харнесс-фикса (не правка вендоренного кода, а
`tests/wpt/config.json` — свой файл проекта): порты `ws`/`wss`/`h2` были
захардкожены в `null` (ни одна прежняя категория не требовала «сырого»
протокольного порта) — включены (`18888`/`18889`/`19000`); `wss`
дополнительно потребовал **локального, некоммитящегося** патча `.venv`
(`pywebsocket3` дергает удалённый в Python 3.12+ `ssl.wrap_socket()`) —
не входит в репозиторий, `tests/wpt/.venv/.gitignore` игнорирует всё.
`ws_doc_root` не имел явного оверрайда и падал на неверный дефолт (тот же
класс готчи, что уже задокументирован для `testdriver.js` — `repo_root`
считается от расположения `tools/`, которое в этом репо лежит в корне
Lumen, а не WPT); починено одной строкой `"ws_doc_root": null` в
`config.json`, включающей верный fallback внутри `tools/serve/serve.py`
самого без правки вендоренного кода.

`run_report.py --all --root websockets --recursive` — ~22 мин,
**342/515 harness OK, 166/934 сабтестов** — реальный сигнал.
`?default` (ws://) реально исполняется и местами проходит; `?wss`/
`?wpt_flags=h2` равномерно падают на уже задокументированный TLS-гэп
(самоподписанный тестовый сертификат, `UnknownIssuer`,
`docs/wpt-status.md:20-28`) — не новая находка. 132 ERROR — тот же
каскад «navigate вернул success, но документ не заменился» на неудачном
TLS-рукопожатии `?wpt_flags=h2` (родственно BUG-438), 132/132 подтверждено
грепом. Из 17 харнесс-TIMEOUT три (`Create-blocked-port.any.html`, все
три варианта) объясняются одной чёткой находкой —
[BUG-772](../../bugs/BUG-772-FIXED.md): `WebSocket` не проверяет список
заблокированных портов спеки Fetch (92 порта), поэтому конструктор реально
пытается TCP-подключиться к каждому вместо синхронного `SecurityError`
— ~230 с суммарно, харнесс роняет файл по внешнему таймауту. Остальные
~14 TIMEOUT (cookies на рукопожатии, `close()` в состоянии CONNECTING,
backpressure `bufferedAmount`, unload-during-open, worker shutdown) —
вероятные отдельные дефекты, **не расследованы** в рамках этой сессии
(время ушло на харнесс-фиксы и BUG-772); задел для следующего захода в
протокольные категории.
