# WPT vendor notes — `webrtc-extensions`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-extensions`, `docs/wpt-status.md`), scope 🚫 ("нет
конвейера" — no pipeline). Same pinned commit `35be3b44`, `git
sparse-checkout add` at the same commit hash, `LICENSE-WPT.md` copied from
the sibling `webrtc`, 13 files (10 glob-counted ids, no `name="variant"`
fan-out, zero `testdriver.js` hits, 2 `.https.*` files). All five
out-of-category dependencies this category pulls
(`webrtc/RTCPeerConnection-helper.js`, `webrtc/RTCConfiguration-helper.js`,
`webrtc/RTCRtpParameters-helper.js`, `webrtc/dictionary-helper.js`,
`webrtc/third_party/sdp/sdp.js`) were already on disk from the earlier
`webrtc` vendoring — no extra fetch needed.

`run_report.py --all --root webrtc-extensions --recursive` (~52 s
wall-clock, single process — too small to need `--processes=4`): **6/10
harness OK, 2/51 subtests passed**. Every unexpected result traces to an
already-documented gap, nothing new:

- **`RTCConfiguration-*.html` + `RTCOAuthCredential.html`** (14 of the 49
  failing subtests): `pc.getConfiguration is not a function` /
  `property "setConfiguration" not found in prototype chain` — the exact
  gap [BUG-721](../../bugs/BUG-721-OPEN.md) already catalogs
  (`RTCPeerConnection.prototype` has no `getConfiguration`/
  `setConfiguration` pair, constructor never validates `iceServers`). This
  category's tests are simply a second, independent surface hitting the
  same missing pair.
- **`RTCRtpTransceiver-headerExtensionControl.html`,
  `RTCRtpParameters-adaptivePtime.html`,
  `RTCRtpSynchronizationSource-{captureTimestamp,senderCaptureTimeOffset}.html`**
  (31 subtests, 1 TIMEOUT, 1 ERROR): all cascade from
  `pc.addTransceiver()`/`getReceivers()`/`getSenders()` being documented
  no-op stubs (`webrtc_stub.rs:242-248`, comment "Stub media/track methods
  — enough to satisfy feature detection") that return `null`/`[]`
  unconditionally — there is no `RTCRtpTransceiver`/`RTCRtpSender`/
  `RTCRtpReceiver` class in the shim at all, so every extensions-API test
  built on top of one throws `Cannot read properties of null/undefined` or
  (when the test's own `t.step_wait` retries the throw) times out. Not a
  new defect — a direct, expected consequence of the stub's stated scope.
- **`RTCRtpCorruptionDetection-headerExtensionControl.html`** (4 subtests):
  `NotAllowedError: Video capture is not available in Lumen Phase 1` —
  expected Phase 1 behavior (no camera capture), not a bug.
  `transfer-datachannel-service-worker.https.html` ERROR and
  `RTCRtpEncodingParameters-scaleResolutionDownTo.https.html` ERROR: both
  "Got results from X, expected Y" / "WebSocket connection closed" —
  the already-documented browsing-context reuse crosstalk
  [BUG-380](../../bugs/BUG-380-OPEN.md), tripped here because the
  preceding TIMEOUT/hung test left stale state before the next
  navigation.

No new `BUG-NNN` filed — same class as `webrtc-encoded-transform`: nothing
of the category's own API to probe once the cascade is traced back to
`addTransceiver`/`getSenders`/`getReceivers` returning nothing, and the one
genuinely reachable surface (`getConfiguration`/`setConfiguration`) already
has an open bug covering it exactly.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «нет конвейера» — вендорена и прогнана целиком 2026-08-09 (коммит
`35be3b44`, `tests/wpt/webrtc-extensions/`, 13 файлов, 10 id по глобу, без
variant-фан-аута, 0 `testdriver.js`). `run_report.py --all --root
webrtc-extensions --recursive` — ~52 с, **6/10 harness OK, 2/51
сабтестов**. Оба кластера падений уже задокументированы: 14 сабтестов —
`pc.getConfiguration`/`setConfiguration` отсутствуют у
`RTCPeerConnection.prototype`, тот же дефект, что
[BUG-721](../bugs/BUG-721-OPEN.md); 31 сабтест (1 TIMEOUT + 1 ERROR) —
каскад от `addTransceiver()`/`getSenders()`/`getReceivers()`, которые в
`webrtc_stub.rs` — документированные no-op заглушки (`null`/`[]`), поэтому
никакого `RTCRtpTransceiver`/`RTCRtpSender`/`RTCRtpReceiver` в шиме вообще
нет — ожидаемое следствие заявленного скоупа заглушки, не новый дефект;
4 сабтеста — `NotAllowedError: Video capture is not available in Lumen
Phase 1` (ожидаемое ограничение Phase 1); 2 ERROR — уже задокументированное
переиспользование browsing context между тестами
([BUG-380](../bugs/BUG-380-OPEN.md)). Новый BUG-NNN не заводился.
