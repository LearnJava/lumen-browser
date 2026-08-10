# WPT vendor notes — `webrtc-stats`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-stats`, `docs/wpt-status.md`). ROADMAP's prior scope note
("WebRTC — нет конвейера") turns out to be accurate in substance for this
specific category — unlike its siblings `webrtc`/`webrtc-extensions`/
`webrtc-priority`, where the same wording was found to be an inaccurate
carry-over (`webrtc_stub.rs` does implement enough signaling-shape API to
produce partial signal there). Here, every test needs an actual event
delivered from one `RTCPeerConnection` instance to its remote counterpart
(`ontrack`, `ondatachannel`, or the packet-flow implied by `inbound-rtp`/
`outbound-rtp` stats), and the stub never wires two instances together at
all — see the new finding below.

Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webrtc`, 8 files total
(`RTCDataChannel-stats.html`, `getStats-remote-candidate-address.html`,
`getStats-remote-candidate-ufrag.html`, `hardware-capability-stats.https.html`,
`idlharness.window.js`, `outbound-rtp.https.html`, `rtp-stats-creation.html`,
`supported-stats.https.html`) + `META.yml`/`README.md`/`WEB_FEATURES.yml`,
8 glob-counted ids, no `name="variant"` fan-out, 1 `testdriver.js` hit
(`idlharness.window.js`, standard IDL-harness boilerplate), 3 `.https.`
files. All files depend on `../webrtc/RTCPeerConnection-helper.js` (already
vendored) and two depend on `../webrtc/RTCDataChannel-helper.js`/
`RTCDataChannel-worker-shim.js` (also already vendored) — no additional
out-of-category vendoring needed.

`run_report.py --all --root webrtc-stats --recursive` (~82 s wall-clock):
**0/8 harness OK, 0/23 subtests passed** — a complete wipeout. 5 files
TIMEOUT directly; the other 3 (`hardware-capability-stats.https.html`,
`outbound-rtp.https.html`, `supported-stats.https.html`) ERROR with
`AssertionError: Got results from <prior test>, expected <this test>` — the
already-documented [BUG-380](../../bugs/BUG-380-FIXED.md) stale-result-reuse
artifact that always follows a TIMEOUT in the browsing context, not an
independent finding.

New finding [BUG-727](../../bugs/BUG-727-OPEN.md): tracing every TIMEOUT
back to source, each hangs on a promise awaiting a **remote**-peer event —
`exchangeOfferAndListenToOntrack` awaits `remotePc.ontrack`;
`openChannelPair`/`RTCDataChannel-stats.html` await `remotePc.ondatachannel`.
`crates/js/src/webrtc_stub.rs`'s `RTCPeerConnection.prototype._dispatch` is
called from exactly one place (`_gatherMdns()`, for `icecandidate` only) —
no code path ever invokes `ontrack`/`ondatachannel`/`onconnectionstatechange`/
`oniceconnectionstatechange`, and each `RTCPeerConnection` instance has no
reference to any other instance, so no event could reach a "remote" peer
even if one were fired. Every WPT test built on the standard two-peer
pattern (`localPc`/`remotePc` exchange, then assert on a remote-side
callback) hangs by construction — this is structural, not a per-property
gap like [BUG-721](../../bugs/BUG-721-OPEN.md)/
[BUG-726](../../bugs/BUG-726-OPEN.md). Root cause of 100% of this
category's TIMEOUTs (5 of 8 files) and plausibly an uncounted contributor
to some already-logged TIMEOUTs in `webrtc`/`webrtc-extensions`/
`webrtc-priority` (not individually re-triaged there — those runs stopped
at their own dominant findings before covering every TIMEOUT).

New `BUG-727` filed (only).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 подтверждён по существу — в отличие от `webrtc`/`webrtc-extensions`/
`webrtc-priority`, где та же формулировка заметки оказалась неточной, здесь
каждый тест требует реального события «с удалённого пира», которого стаб
никогда не порождает. Вендорена целиком 2026-08-09 (коммит `35be3b44`,
`tests/wpt/webrtc-stats/`, 8 файлов, 8 id по глобу, без variant-фан-аута,
1 `testdriver.js` (idlharness), 3 `.https.`). `run_report.py --all --root
webrtc-stats --recursive` — ~82 с, **0/8 harness OK, 0/23 сабтестов**,
стопроцентный отказ. Найден [BUG-727](../bugs/BUG-727-OPEN.md): стаб
диспатчит `_dispatch` только для `icecandidate` (из `_gatherMdns()`) —
`ontrack`/`ondatachannel`/`on(ice)connectionstatechange` не вызываются
никогда, а инстансы `RTCPeerConnection` не связаны друг с другом вовсе, так
что любой канонический двухпировый тест виснет до таймаута враннера. Три
файла из восьми дополнительно дают `AssertionError` (уже известный дрейф
[BUG-380](../bugs/BUG-380-FIXED.md) — переиспользование результата
предыдущего теста после TIMEOUT), не самостоятельная находка. Новый номер:
BUG-727.
