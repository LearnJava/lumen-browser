# WPT vendor notes — `webrtc-priority`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-priority`, `docs/wpt-status.md`). ROADMAP's prior scope
note ("WebRTC — нет конвейера") was inaccurate, same drift already found on
the parent `webrtc` and on `webrtc-extensions`/`webrtc-ice`: `webrtc_stub.rs`
does implement an `RTCPeerConnection`/`RTCDataChannel` mDNS-only stub, it
just doesn't implement `RTCRtpSender.setParameters`/`getParameters`
encoding-priority fields or `RTCDataChannel`'s `priority` option.

Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webrtc`, 2 files total
(`RTCPeerConnection-ondatachannel.html`, `RTCRtpParameters-encodings.html`)
+ `META.yml`, 2 glob-counted ids, no `name="variant"` fan-out, zero
`testdriver.js` hits, no `.https.` files. Both files depend on helper
scripts already vendored under `tests/wpt/webrtc/`
(`RTCPeerConnection-helper.js`, `dictionary-helper.js`,
`RTCRtpParameters-helper.js`) — no additional out-of-category vendoring
needed.

`run_report.py --all --root webrtc-priority --recursive` (~27 s
wall-clock): **2/2 harness OK, 0/9 subtests passed**.

`RTCPeerConnection-ondatachannel.html` (2 subtests, both FAIL): both fail on
their very first assertion, `assert_equals(dc1.priority, 'high'/'low')` —
checking the property on the **local** `RTCDataChannel` object returned
directly by `pc1.createDataChannel(label, options)`, before the test even
reaches the remote-peer `ondatachannel` check. New finding
[BUG-726](../../bugs/BUG-726-OPEN.md): `createDataChannel`'s implementation
in `WEBRTC_SHIM` takes only `label`, silently dropping the second
`RTCDataChannelInit` argument — the returned object has no `priority`
field at all (nor `ordered`/`protocol`/`maxRetransmits`/etc.), so `.priority`
reads back `undefined` instead of the spec default `'low'` or the
explicitly-passed `'high'`.

`RTCRtpParameters-encodings.html` (7 subtests, all FAIL): every subtest
destructures `{ sender }` from `pc.addTransceiver(...)`, which returns
`null` (the shim's `addTransceiver` is an unconditional no-op stub) — the
destructure throws `TypeError` before any encoding/priority logic runs.
Same root cause already filed as [BUG-721](../../bugs/BUG-721-OPEN.md)'s
sibling gap (`webrtc-extensions`' note: "no-op заглушки
addTransceiver/getSenders/getReceivers"), not a distinct defect worth its
own number.

New `BUG-726` filed (only).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 (вне скоупа, заметка «нет конвейера» была неточна — тот же дрейф,
что у `webrtc`/`webrtc-extensions`). Вендорена целиком 2026-08-09 (коммит
`35be3b44`, `tests/wpt/webrtc-priority/`, 2 файла, 2 id по глобу, без
variant-фан-аута, 0 `testdriver.js`, без `.https.`). `run_report.py --all
--root webrtc-priority --recursive` — ~27 с, **2/2 harness OK, 0/9
сабтестов**. Найден [BUG-726](../../bugs/BUG-726-OPEN.md):
`createDataChannel(label, options)` отбрасывает `options` целиком —
возвращаемый `RTCDataChannel` не отражает ни `priority`, ни любое другое
поле `RTCDataChannelInit`, `dc1.priority` читается как `undefined` вместо
спекового значения по умолчанию `'low'`/явно переданного `'high'`. Остальные
7 сабтестов падают на уже задокументированном
[BUG-721](../bugs/BUG-721-OPEN.md)-смежном пробеле — `addTransceiver()`
безусловно отдаёт `null`. Новый номер: BUG-726.
