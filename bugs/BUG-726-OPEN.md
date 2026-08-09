# BUG-726 — `RTCPeerConnection.createDataChannel()` ignores its `options` dictionary entirely — no `priority`, `ordered`, `protocol`, `maxRetransmits`, etc. reflected

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webrtc_stub.rs` — `WEBRTC_SHIM`)
**Найден:** P2, WPT-VENDOR-webrtc-priority, 2026-08-09

## Симптом

`run_report.py --all --root webrtc-priority --recursive`: 2/2 harness OK,
**0/9 subtests passed**. `RTCPeerConnection-ondatachannel.html`'s very first
assertion fails immediately:

```
FAIL In-band negotiated channel created on remote peer should match the same
configuration as local peer - assert_equals: expected (string) "high" but
got (undefined) undefined
FAIL In-band negotiated channel created on remote peer should match the same
(default) configuration as local peer - assert_equals: expected (string)
"low" but got (undefined) undefined
```

Both failures are `assert_equals(dc1.priority, 'high' / 'low')` — the
assertion on the **local** channel returned directly by `createDataChannel`,
before the test even reaches the `pc2.ondatachannel` remote-side check. Live
probe confirms:

```js
new RTCPeerConnection().createDataChannel('x', {priority: 'high', ordered: false,
  protocol: 'custom', maxRetransmits: 1}).priority   // → undefined, spec: 'high'
```

`RTCRtpParameters-encodings.html`'s remaining 7 subtests all fail on the
already-documented [BUG-721](BUG-721-OPEN.md)-adjacent gap —
`pc.addTransceiver()` returns `null` (no-op stub, per
`WPT-VENDOR-webrtc-extensions`'s note), so `sender` destructuring throws —
not a new finding, just the addTransceiver no-op paying its price here too.

## Причина

`crates/js/src/webrtc_stub.rs`, `RTCPeerConnection.prototype.createDataChannel`:

```js
RTCPeerConnection.prototype.createDataChannel = function(label) {
    return {
      label: label || '',
      readyState: 'connecting',
      bufferedAmount: 0,
      send: function() {},
      close: function() {},
      onopen: null, onmessage: null, onerror: null, onclose: null,
      addEventListener: function() {}, removeEventListener: function() {}
    };
  };
```

The function signature drops the second `options` argument outright — every
`RTCDataChannelInit` field (`ordered`, `maxPacketLifeTime`,
`maxRetransmits`, `protocol`, `negotiated`, `id`, `priority`) is discarded,
and the returned object exposes none of them as properties at all (not even
with wrong values — the fields are simply absent, so `.priority` reads back
`undefined` instead of the spec default `'low'`).

## Дальше

Fix scope: accept the `options` parameter, copy each `RTCDataChannelInit`
field onto the returned object with its per-spec default
(`ordered: true`, `protocol: ''`, `negotiated: false`, `priority: 'low'`,
`maxPacketLifeTime`/`maxRetransmits`/`id`: `null`/unset), and also add a
`label` getter (currently a plain data property, spec-correct enough for
this stub's scope). No signaling/negotiation is needed to fix this specific
finding — both failing assertions in this category check the **local**
channel's own reflected properties, not a value renegotiated with the
remote peer.
