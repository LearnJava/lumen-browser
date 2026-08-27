# WPT vendor notes — `webrtc-svc`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-17 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-svc`, `docs/wpt-status.md`), scope 🚫 ("нет конвейера" —
no pipeline). Same pinned commit `35be3b44`, `git sparse-checkout add` at
the same commit hash, `LICENSE-WPT.md` copied from a sibling `webrtc-*`
category, 6 files (5 test `.html` + `svc-helper.js`, no `name="variant"`
fan-out, zero `testdriver.js` hits, no `.https.*` files). The category's
own out-of-category dependencies (`webrtc/dictionary-helper.js`,
`webrtc/RTCRtpParameters-helper.js`, `../webrtc/RTCPeerConnection-helper.js`)
were already on disk from the earlier `webrtc` vendoring.

Confirmed the ROADMAP note's scope call **before** vendoring, per the
family-drift rule (`webrtc`/`webrtc-priority`/`webrtc-extensions` had a
stale "нет конвейера" note; `webrtc-ice`/`webrtc-identity` did not) — grepped
`crates/js/src/webrtc_stub.rs` for `scalabilityMode`/`sendEncodings`/
`getParameters`/`setParameters`/`encodingInfo`/`mediaCapabilities`: only
`addTransceiver` exists, and it is the documented `return null;` no-op
stub. `navigator.mediaCapabilities.encodingInfo()` does exist
(`crates/js/src/media_capabilities.rs`), but is a Phase 0 stub that
resolves `{supported:true, smooth:true, powerEfficient:false}` for any
config with a string `.type` — already documented as such in the module's
own doc comment, not a hidden gap.

`run_report.py --all --root webrtc-svc --recursive` (~41 s wall-clock,
single process): **5/5 harness OK, 0/67 subtests passed**. Every subtest
failure traces to one of two already-documented gaps, nothing new:

- **63 subtests** (`RTCRtpParameters-scalability-{av1,h264,vp8,vp9}.html`,
  every `L*T*`/`S*T*` scalability-mode variant): `NotAllowedError: Video
  capture is not available in Lumen Phase 1` — expected Phase 1 behavior
  (no camera/`getUserMedia` capture), not a bug.
- **4 subtests** (`RTCRtpParameters-scalability.html`): `pc.addTransceiver`
  returning `null` unconditionally (`webrtc_stub.rs:243`) — the same
  documented no-op stub already covered by
  [BUG-721](../../bugs/BUG-721-OPEN.md)/[BUG-726](../../bugs/BUG-726-OPEN.md).
  Two subtests throw `TypeError: Cannot destructure property 'sender' of
  'pc.addTransceiver(...)' as it is null.`, one throws `TypeError: Cannot
  read properties of null (reading 'sender')`, and one
  (`Setting a scalability mode to nonsense throws an exception`) fails the
  opposite way — `assert_throws_dom` expected an `OperationError` that
  never fires because `addTransceiver` returns before any validation runs.

No new `BUG-NNN` filed — the category has no API surface of its own beyond
`RTCRtpParameters.encodings[].scalabilityMode`, and every reachable path
into it already dead-ends at the documented `addTransceiver` stub.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «нет конвейера» подтверждён точно перед вендорингом (грепом
`webrtc_stub.rs` — из проверяемого API есть только `addTransceiver`,
документированная заглушка `return null;`; `scalabilityMode`/
`sendEncodings`/`getParameters`/`setParameters` отсутствуют вовсе).
Вендорена целиком 2026-08-17 (коммит `35be3b44`, `tests/wpt/webrtc-svc/`,
6 файлов, 5 id, без variant-фан-аута, без `.https.`). `run_report.py --all
--root webrtc-svc --recursive` — ~41 с, **5/5 harness OK, 0/67
сабтестов**. Оба кластера падений уже задокументированы: 63 сабтеста —
`NotAllowedError: Video capture is not available in Lumen Phase 1`
(ожидаемое ограничение Phase 1, нет захвата камеры); 4 сабтеста — каскад от
`addTransceiver()`, возвращающего `null` безусловно, тот же дефект, что
[BUG-721](../bugs/BUG-721-OPEN.md)/[BUG-726](../../bugs/BUG-726-OPEN.md).
Новый BUG-NNN не заводился.
