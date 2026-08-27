# WPT vendor notes — `webrtc`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc`, `docs/wpt-status.md`), scope 🚫 in ROADMAP's original
note ("нет конвейера" — no pipeline) — **incorrect on inspection**, same
class of drift as `webmidi`: `crates/js/src/webrtc_stub.rs` implements a real
mDNS-only privacy stub (`RTCPeerConnection`/`RTCSessionDescription`/
`RTCIceCandidate`, W3C WebRTC §9D.5 IP-leak mitigation), not an absent API.
Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webmidi` category, 259 files
(224 glob-counted ids; `coverage/`, `resources/`, `tools/`, `third_party/`
hold no direct tests). Predictors: 17/259 files pull `testdriver.js` (~7%,
low), 99 `.https.*` files, 33 `name="variant"` hits (fan-out materialized:
wptrunner ran 258 instances, not 224).

`run_report.py --all --root webrtc --recursive --processes=4` (~17:42
wall-clock): **102/258 harness OK, 86/1126 subtests passed**. 112 harness
outcomes (60 TIMEOUT + 52 navigate ERROR) are the already-documented TLS gap
([BUG-657](../../bugs/BUG-657-OPEN.md), `UnknownIssuer` — grep count of
`UnknownIssuer` in the run log matches exactly). Of the remaining signal, the
two largest failure clusters (66× `setConfiguration(config)` + 63×
`new RTCPeerConnection(config)`, 129 of 1040 unpassed subtests) both trace to
one gap: `setConfiguration`/`getConfiguration` are entirely absent from
`RTCPeerConnection.prototype`, and the constructor performs no
`RTCConfiguration` validation at all. A live probe (`--mcp-live-port`)
confirmed both directly (`typeof RTCPeerConnection.prototype.setConfiguration
=== 'undefined'`, and `new RTCPeerConnection({iceServers:[{urls:'not-a-valid-
url'}]})` does not throw) and ruled out the BUG-629/374/672/713/719
guard-less-constructor pattern (`RTCPeerConnection()` without `new` correctly
throws `TypeError`). Filed as [BUG-721](../../bugs/BUG-721-OPEN.md).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп был отмечен в ROADMAP как 🚫 «нет конвейера» — при вендоринге
выяснилось, что это неточно: `crates/js/src/webrtc_stub.rs` реализует
настоящую mDNS-only заглушку приватности (§9D.5 W3C WebRTC — не даёт
утечь реальному IP через ICE-кандидаты), а не полное отсутствие API. Та же
категория дрейфа, что была найдена у `webmidi`. Вендорена целиком 2026-08-09
(коммит `35be3b44`, `tests/wpt/webrtc/`, 259 файлов, 224 id по глобу).
`run_report.py --all --root webrtc --recursive --processes=4` — ~17:42,
258 реально исполненных инстансов (variant-фан-аут): **102/258 harness OK,
86/1126 сабтестов**. 112 исходов (60 TIMEOUT + 52 ERROR при навигации) —
уже задокументированный TLS-гэп [BUG-657](../../bugs/BUG-657-OPEN.md)
(подтверждено точным совпадением с числом строк `UnknownIssuer` в логе).
Два крупнейших кластера падений (66× `setConfiguration(config)` + 63×
`new RTCPeerConnection(config)`, 129 из 1040 непройденных сабтестов) —
один и тот же дефект: у `RTCPeerConnection.prototype` вовсе нет
`setConfiguration`/`getConfiguration`, а конструктор не валидирует
`RTCConfiguration.iceServers` (ни `SyntaxError` на битый URL, ни
`InvalidAccessError` на слишком длинный TURN username, ни `TypeError` на
`null`). Живая проба подтвердила оба факта напрямую и опровергла паттерн
guard-less-конструктора (`RTCPeerConnection()` без `new` корректно бросает
`TypeError`) — заведён [BUG-721](../bugs/BUG-721-OPEN.md).
