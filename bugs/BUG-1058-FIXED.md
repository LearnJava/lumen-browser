# BUG-1058 — `RTCPeerConnection` has no `setConfiguration`/`getConfiguration`, and the constructor never validates `RTCConfiguration`

**Статус:** FIXED 2026-09-18
**Компонент:** js (`crates/js/src/webrtc_stub.rs` — `WEBRTC_SHIM`)
**Найден:** P2, WPT-VENDOR-webrtc, 2026-08-09

**Заведён как BUG-721** — при разборе оказалось, что номер уже занят закрытым
багом (`fetch()` отдавал тело чужого ответа, 2026-08-09), заведённым позже
того же дня. Перенумерован в BUG-1058 при закрытии 2026-09-18; `BUG-721` в
любой ссылке ниже — старое имя этой заявки, не смежная.

## Симптом

`run_report.py --all --root webrtc --recursive --processes=4`: 102/258 harness
OK, 86/1126 subtests passed. The two single largest failure clusters in the
log (129 of the non-TLS-gap failures) are:

```
66  setConfiguration(config) - ... - assert_idl_attribute: property "setConfiguration" not found in prototype chain
    (or, once past IDL detection: TypeError: pc.setConfiguration is not a function)
63  new RTCPeerConnection(config) - ... - assert_throws_dom: ... did not throw
```

Live probe (`--mcp-live-port`, page with a `<script>` so a JS context exists):

```json
{
  "hasSetConfiguration": "undefined",
  "hasGetConfiguration": "undefined",
  "ctorNoNewThrows": "TypeError",
  "invalidUrlThrows": "NO_THROW",
  "protoOwnProps": 26,
  "instanceOwnProps": 17,
  "symbolToStringTag": "[object Object]"
}
```

`new RTCPeerConnection({iceServers:[{urls:'not-a-valid-url'}]})` succeeds
silently instead of throwing `SyntaxError`. (`ctorNoNewThrows: "TypeError"`
confirms the constructor *is* new-guarded — this is not another instance of
the BUG-629/374/672/713/719 guard-less-constructor pattern; the gap here is
config validation, not construction guarding.)

## Масштаб

Every WPT file under `webrtc/RTCConfiguration-*.html` and every
`setConfiguration(...)` subtest across the category fails on this — the two
clusters above account for ~12% of all 1040 unpassed subtests in the run,
making it the dominant functional (non-BUG-657-TLS-gap) finding in the
category. `CAPABILITIES.md` does not list WebRTC at all (the mDNS-only stub
is intentionally minimal per its own module doc comment — see
`crates/js/src/webrtc_stub.rs:1-16`), so this is a gap against the stub's own
stated scope (feature-detection + signaling-shape fidelity), not a missing
"nice to have".

## Причина

`crates/js/src/webrtc_stub.rs` (`WEBRTC_SHIM`, `RTCPeerConnection` starting
at line 126):
- The constructor (line 126-145) stores `config` verbatim as `this._config`
  with no validation of `iceServers` (URL scheme allow-list, TURN username
  length ≤ 512 UTF-16 code units, `InvalidAccessError`/`SyntaxError`/
  `TypeError` per the WebRTC spec's `RTCPeerConnection(configuration)`
  constructor steps).
- `RTCPeerConnection.prototype` (lines 147-260) defines getters for
  `localDescription`/`remoteDescription`/`signalingState`/
  `iceGatheringState`/`iceConnectionState`/`connectionState` and methods
  `addEventListener`/`removeEventListener`/`dispatchEvent`/`createOffer`/
  `createAnswer`/`setLocalDescription`/`setRemoteDescription`/
  `addIceCandidate`/`close`/`addTransceiver`/`addTrack`/`removeTrack`/
  `getTransceivers`/`getSenders`/`getReceivers`/`getStats`/
  `createDataChannel` — no `setConfiguration`/`getConfiguration` pair exists
  anywhere in the shim.

## Фикс

`WEBRTC_SHIM` (`crates/js/src/webrtc_stub.rs`) gets `getConfiguration()`
(shallow copy of `this._config` merged with spec defaults) and
`setConfiguration(config)` (merges into `this._config`, throws
`InvalidModificationError` DOMException on a `certificates` change) on
`RTCPeerConnection.prototype`. A shared `_validateIceServers(iceServers)`
helper is called from both the constructor and `setConfiguration`: `null` →
`TypeError`; any `urls` entry whose scheme isn't `stun:`/`stuns:`/`turn:`/
`turns:` → `SyntaxError` DOMException; a TURN `username` over 512 UTF-16
code units → `InvalidAccessError` DOMException.

6 new unit tests (`crates/js/src/webrtc_stub.rs::tests`) cover
`getConfiguration` reflecting the constructor arg, `setConfiguration`
merging, and all three validation error paths plus the
`InvalidModificationError` rejection.
`cargo test -p lumen-js --lib webrtc_stub:: --features v8-backend` — 23/23.
`cargo clippy --workspace --all-targets -- -D warnings` — чист.
`scripts/scoped-test.sh` — единственный красный
`cases::snapshot_cpu::cpu_snapshots_match_references` — чужой дрейф эталонов
([BUG-1008](BUG-1008-OPEN.md), тот же набор из 7 файлов), не связан с правкой
(`git status --short` перед коммитом — изменён только `webrtc_stub.rs`).
