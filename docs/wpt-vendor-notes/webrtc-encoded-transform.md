# WPT vendor notes — `webrtc-encoded-transform`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-encoded-transform`, `docs/wpt-status.md`), scope 🚫
("нет конвейера" — no pipeline). Unlike the sibling `webrtc` category (whose
🚫 note turned out to be inaccurate — `webrtc_stub.rs` implements a real
mDNS-only `RTCPeerConnection` stub), this note checks out: the encoded-
transform extension surface (`RTCRtpScriptTransform`, `RTCRtpSender`/
`RTCRtpReceiver.prototype.createEncodedStreams`, `RTCEncodedAudioFrame`/
`RTCEncodedVideoFrame`, `SFrameEncrypterStream`/`SFrameDecrypterStream`/
`RTCRtpSFrameEncrypter`) is entirely absent from `webrtc_stub.rs` — that file
only defines `RTCPeerConnection`/`RTCSessionDescription`/`RTCIceCandidate`,
plus no-op `addTransceiver`/`addTrack`/`getSenders`/`getReceivers` (the last
two return `[]`, so there is nothing to hang a sender/receiver-scoped API
off in the first place).

Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webrtc` category, 62 files
(38 glob-counted ids, no `name="variant"` fan-out). Predictors: 28/62 files
pull `testdriver.js` (~45%), 30 `.https.*` files.

`run_report.py --all --root webrtc-encoded-transform --recursive
--processes=4` (~8:11 wall-clock): **7/38 harness OK, 0/21 subtests
passed**. Breakdown of the 31 unexpected results: 27 TIMEOUT (mostly the
already-documented TLS gap [BUG-657](../../bugs/BUG-657-OPEN.md),
`UnknownIssuer` on `.https.` navigation) and 4 ERROR (the already-documented
session-reuse crosstalk [BUG-380](../../bugs/BUG-380-FIXED.md) — "Got results
from X, expected Y" on the four `RTC*Frame-clone`/`-metadata`/
`insertable-streams-audio` files that ran back-to-back). Every one of the
21 subtests that actually executed failed with a plain `ReferenceError`
(`RTCRtpScriptTransform is not defined`, `SFrameEncrypterStream is not
defined`, etc.) — consistent total absence, not a broken partial stub, so
no new BUG-NNN was filed (same class as `fenced-frame`: nothing of the
category's own API to probe).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «нет конвейера» подтверждён точно на этот раз (в отличие от
родительской `webrtc`, где та же заметка оказалась неточной из-за
mDNS-заглушки в `webrtc_stub.rs`): весь API категории — `RTCRtpScriptTransform`,
`createEncodedStreams()` на `RTCRtpSender`/`RTCRtpReceiver`,
`RTCEncodedAudioFrame`/`RTCEncodedVideoFrame`, `SFrameEncrypterStream`/
`SFrameDecrypterStream`/`RTCRtpSFrameEncrypter` — отсутствует в
`crates/js/src/webrtc_stub.rs` целиком; там реализованы только
`RTCPeerConnection`/`RTCSessionDescription`/`RTCIceCandidate` с
заглушками `addTransceiver`/`addTrack`/`getSenders`/`getReceivers`
(последние две всегда возвращают `[]`, так что навешивать sender/receiver
API попросту не на что). Вендорена целиком 2026-08-09 (коммит `35be3b44`,
`tests/wpt/webrtc-encoded-transform/`, 62 файла, 38 id по глобу, без
variant-фан-аута). `run_report.py --all --root webrtc-encoded-transform
--recursive --processes=4` — ~8:11, **7/38 harness OK, 0/21 сабтестов**.
Из 31 неожиданного исхода 27 — TIMEOUT (в основном уже задокументированный
TLS-гэп [BUG-657](../../bugs/BUG-657-OPEN.md), `UnknownIssuer` на `.https.`-
навигации) и 4 — ERROR (уже задокументированное переиспользование
результатов сессии, [BUG-380](../../bugs/BUG-380-FIXED.md): «Got results from
X, expected Y» на четырёх файлах `RTC*Frame-clone`/`-metadata`/
`insertable-streams-audio`, исполнившихся подряд). Все 21 реально
исполнившихся сабтеста падают на простой `ReferenceError: <API> is not
defined` — согласованное полное отсутствие, а не сломанный частичный стаб,
поэтому новый BUG-NNN не заводился (тот же класс, что `fenced-frame`: у
категории нет собственного API, который стоило бы пробовать живьём).
