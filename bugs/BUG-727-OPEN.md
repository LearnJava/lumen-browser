# BUG-727 — `RTCPeerConnection` stub never fires `ontrack`/`ondatachannel`/`on(ice)connectionstatechange` — any two-peer WPT test hangs to the harness timeout

**Статус:** OPEN (ДОРАБОТКА → [GAP-WEBRTC](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-WEBRTC` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Компонент:** js (`crates/js/src/webrtc_stub.rs` — `WEBRTC_SHIM`)
**Найден:** P2, WPT-VENDOR-webrtc-stats, 2026-08-09

## Симптом

`run_report.py --all --root webrtc-stats --recursive`: **0/8 harness OK,
0/23 subtests** — every file either TIMEOUTs or ERRORs (the ERRORs are the
already-known [BUG-380](BUG-380-FIXED.md) stale-result-reuse artifact that
follows a TIMEOUT). Representative failures:

```
TIMEOUT /webrtc-stats/rtp-stats-creation.html
  TIMEOUT No RTCRtpStreamStats exist when only remote description is set - Test timed out
TIMEOUT /webrtc-stats/getStats-remote-candidate-address.html
  TIMEOUT Do not expose in stats remote addresses that are not known to be already exposed to JS - Test timed out
TIMEOUT /webrtc-stats/RTCDataChannel-stats.html
TIMEOUT /webrtc-stats/idlharness.window.html
```

Every hang traces to a helper that awaits an event on the **remote** peer
after the local peer acts:
`exchangeOfferAndListenToOntrack` awaits `remotePc.ontrack`;
`openChannelPair`/`RTCDataChannel-stats.html` await `remotePc.ondatachannel`.
Neither event, nor `onconnectionstatechange`/`oniceconnectionstatechange`,
ever fires, so these promises never settle and the test runs to the
wptrunner-level timeout instead of the test's own bounded wait.

## Причина

`crates/js/src/webrtc_stub.rs`, `RTCPeerConnection.prototype._dispatch` is
called from exactly one place — `_gatherMdns()` (`this._dispatch('icecandidate', evt)`,
twice, for the synthetic candidate + end-of-candidates). No other code path
in the shim ever calls `_dispatch` or invokes `this.ontrack`/`this.ondatachannel`/
`this.onconnectionstatechange`/`this.oniceconnectionstatechange` directly:

- `addTrack`/`addTransceiver` are no-ops returning `null` — no `track` event
  is synthesized on either peer (own finding, see [BUG-721](BUG-721-OPEN.md)/
  [BUG-726](BUG-726-OPEN.md) for the object-shape half of the same methods).
- `createDataChannel` returns a plain detached object with its own dead
  `addEventListener`/`onopen` — it is never associated with the *other*
  peer's connection, so `remotePc.ondatachannel` has no trigger at all.
- `connectionState`/`iceConnectionState` getters (lines 152-153) are backed
  by `this._closed`/`_iceConnState`, but `_iceConnState` is initialized to
  `'new'` in the constructor and never reassigned anywhere in the file except
  `close()` (→ `'closed'`) — so `oniceconnectionstatechange` has no state
  transition to report even if something called it, and nothing does.

This is a structural gap, not a per-property omission like BUG-721/726: the
stub models each `RTCPeerConnection` as fully independent (per its own
module doc, `crates/js/src/webrtc_stub.rs:1-16` — deliberately no real
media/candidate exchange, mDNS-privacy-only scope), so there is no code path
that could deliver an event *between* two peer instances. Any WPT test
built on the standard two-peer pattern (`localPc`/`remotePc` exchanging
description + candidates, then asserting on a remote-side callback) hangs
by construction, independent of which category exercises it.

## Масштаб

Confirmed as the root cause of 100% of `webrtc-stats`'s TIMEOUTs (5 of 8
files use `ontrack` or a data-channel-pair helper). The same mechanism is
very likely under some of the un-investigated TIMEOUTs already logged
against `webrtc`/`webrtc-extensions`/`webrtc-priority` (not re-verified
here — those runs stopped at their own dominant findings, BUG-721/726,
before individually triaging every remaining TIMEOUT), so this may already
be an uncounted contributor there too.

## Дальше

Fix scope: on `setRemoteDescription`, if the connection now has a matched
local `createDataChannel`/`addTrack`/`addTransceiver` state from the peer
that supplied the description, synthesize `ondatachannel`/`ontrack` on
`this` — this requires the two stub instances to reference each other (or a
shared out-of-band channel), which the current design deliberately avoids.
Given the module's stated privacy-only scope (no real media pipeline), the
cheaper fix may be documentation-only: mark `webrtc-stats` (and any other
category whose tests are two-peer-event-shaped rather than
signaling-shape-only) as intentionally 🚫 in `docs/wpt-status.md`, and note
in the module doc comment that `ontrack`/`ondatachannel`/`on(ice)connectionstatechange`
are out of scope by design — leave the actual cross-peer wiring to a future
real-transport implementation, not this stub.

Найден P2, WPT-VENDOR-webrtc-stats, 2026-08-09.
