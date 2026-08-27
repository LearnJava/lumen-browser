# WPT vendor notes — `webrtc-ice`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-ice`, `docs/wpt-status.md`), scope 🚫 ("нет конвейера" —
no pipeline). Unlike the parent `webrtc` category (whose 🚫 note was
inaccurate — `webrtc_stub.rs` implements a real mDNS-only `RTCPeerConnection`
stub), this note checks out: the entire tested surface is the standalone
`RTCIceTransport` interface (constructor, `role`/`state`/`gatheringState`,
`gather()`/`start()`/`stop()`/`addRemoteCandidate()`,
`getLocalCandidates()`/`getRemoteCandidates()`/`getSelectedCandidatePair()`,
`icecandidate`/`gatheringstatechange`/`statechange`/
`selectedcandidatepairchange` events) — `grep -n RTCIceTransport
crates/js/src/webrtc_stub.rs` finds zero matches. `webrtc_stub.rs` only
defines `RTCPeerConnection`/`RTCSessionDescription`/`RTCIceCandidate`; ICE
gathering there is entirely internal to `RTCPeerConnection` (`_iceGatheringState`/
`_iceConnState` fields, synthetic single mDNS candidate on `onicecandidate`)
and never exposed as a standalone transport object a page can construct.

Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webrtc`, 3 files total
(1 test file `RTCIceTransport-extension.https.html` + 1 helper
`RTCIceTransport-extension-helper.js` + `META.yml`, 1 glob-counted id, no
`name="variant"` fan-out, zero `testdriver.js` hits, the one test file is
`.https.`).

`run_report.py --all --root webrtc-ice --recursive` (~45 s wall-clock):
**0/1 harness OK**. The single id TIMEOUTs on the already-documented TLS
gap [BUG-657](../../bugs/BUG-657-OPEN.md) (`UnknownIssuer` on `.https.`
navigation) before any page script runs — no functional signal from the
run itself, but the constructor's total absence (confirmed by the source
grep above) means the result would be a harness-level `ReferenceError`
regardless of the TLS gap. No new `BUG-NNN` filed — same class as
`webrtc-encoded-transform`/`webrtc-extensions`: nothing of the category's
own API exists to probe live.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «нет конвейера» подтверждён точно (в отличие от родительской
`webrtc`, где та же заметка оказалась неточной из-за mDNS-заглушки в
`webrtc_stub.rs`): весь тестируемый API категории — самостоятельный
интерфейс `RTCIceTransport` (конструктор, `role`/`state`/
`gatheringState`, `gather()`/`start()`/`stop()`/`addRemoteCandidate()`,
геттеры кандидатов/пары, события `icecandidate`/`gatheringstatechange`/
`statechange`/`selectedcandidatepairchange`) — отсутствует в
`crates/js/src/webrtc_stub.rs` целиком (`grep -n RTCIceTransport
webrtc_stub.rs` — ноль совпадений); ICE-состояние там существует только
как внутренние поля `RTCPeerConnection` (`_iceGatheringState`/
`_iceConnState`, один синтетический mDNS-кандидат), отдельного
конструируемого транспорта нет. Вендорена целиком 2026-08-09 (коммит
`35be3b44`, `tests/wpt/webrtc-ice/`, 3 файла, 1 id по глобу, без
variant-фан-аута, 0 `testdriver.js`). `run_report.py --all --root
webrtc-ice --recursive` — ~45 с, **0/1 harness OK**: единственный id
падает TIMEOUT на уже задокументированном TLS-гэпе
[BUG-657](../../bugs/BUG-657-OPEN.md), сигнала из самого прогона нет, но
подтверждённое грепом полное отсутствие `RTCIceTransport` означает, что
и без TLS-гэпа тест упал бы `ReferenceError`. Новый BUG-NNN не заводился.
