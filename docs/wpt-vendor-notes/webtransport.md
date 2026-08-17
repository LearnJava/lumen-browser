# WPT vendor notes — `webtransport`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webtransport`, `docs/wpt-status.md`), scope 🚫 ("нет транспортного
стека" — no transport stack). Same pinned commit `35be3b44`, `git
sparse-checkout add` at the same commit hash, `LICENSE-WPT.md` copied from a
sibling category, 43 files (25 test `.html`/`.js` + `META.yml`/`README.md`/
`WEB_FEATURES.yml` + `handlers/`/`resources/` helper dirs).

Confirmed the ROADMAP note's scope call **before** vendoring, per the
family-drift rule (`webrtc`/`webrtc-priority`/`webrtc-extensions` had a stale
"нет конвейера" note; some siblings did not) — grepped
`crates/js/src/webtransport.rs`: the whole file is a Phase-0 stub, every
operation `reject`/`throw`s `WebTransportError('… no QUIC …',
'phase-0-stub')` (`get ready` rejects, `createBidirectionalStream`/
`createUnidirectionalStream` reject, `datagrams.readable/writable.read/write`
throw). `crates/network/src/h3/` is pure QUIC/HTTP-3 **codecs** with no live
connection loop, socket, or Extended CONNECT — confirmed against
`docs/tasks/ph3-webtransport.md` (P1's own brief for this future task),
current as of this vendoring. Scope call stands: 🚫, and unlike `webrtc-svc`
(a real, if incomplete, JS surface) `webtransport` has no reachable success
path at all — every test necessarily fails before touching the stub, since
`new WebTransport(url)` itself rejects `ready` immediately.

`run_report.py --all --root webtransport --recursive` (~8 min wall-clock,
single process, venv python — the system python picked up an unpatched
`pywebsocket3` and needed `.venv/Scripts/python.exe` explicitly): **0/24
harness OK, 0/0 subtests** (one vendored file,
`bidirectional-cancel-crash.https.html`, is a manual/non-testharness page and
is not counted as an id). All 24 failures are `TIMEOUT`, and all 24 show the
same navigation-time failure in the Lumen log: `network error: TLS
handshake: invalid peer certificate: UnknownIssuer`.

This is **not** a webtransport-specific finding — it is the pre-existing,
already-documented TLS-trust gap from WPT-RUN-2 (`tests/wpt/certs/README.md`:
"This cert is not trusted by Lumen's own TLS client … a `.https.` test
currently fails fast with `TLS handshake: invalid peer certificate:
UnknownIssuer`… That is the documented, expected residual of WPT-RUN-2's
HTTPS-port half", also cited as "TLS-гэп" [BUG-657](../../bugs/BUG-657-OPEN.md)
in sibling categories' notes). Every file in this category is `.https.*` (WebTransport
requires HTTPS/QUIC by spec), so the category hits the gap at a 100% rate —
the harness never gets far enough to exercise the `webtransport.rs` stub
itself. No new `BUG-NNN` filed: the category has no reachable API surface
(confirmed 🚫 above) and the one signal it did produce is the pre-existing,
already-tracked TLS gap, not a new defect.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «нет транспортного стека» подтверждён точно перед вендорингом
(грепом `crates/js/src/webtransport.rs` — весь файл Phase-0 заглушка, все
операции reject/throw `WebTransportError('… no QUIC …', 'phase-0-stub')`;
`crates/network/src/h3/` — чистые кодеки без живого QUIC-соединения).
Вендорена целиком 2026-08-18 (коммит `35be3b44`, `tests/wpt/webtransport/`,
43 файла, 24 id, без variant-фан-аута). `run_report.py --all --root
webtransport --recursive` (~8 мин, через venv python — системный python
подхватывал непатченный `pywebsocket3`) — **0/24 harness OK, 0/0
сабтестов**. Все 24 исхода — `TIMEOUT` на одном и том же
уже задокументированном TLS-гэпе (`tests/wpt/certs/README.md`,
[BUG-657](../bugs/BUG-657-OPEN.md)): `UnknownIssuer` при загрузке `.https.`
страницы, до всякого обращения к `webtransport.rs`. Не находка категории —
вся категория состоит из `.https.`-файлов (WebTransport требует HTTPS/QUIC
по спеке), поэтому попадает в гэп со 100%-й вероятностью. Новый BUG-NNN не
заводился.
