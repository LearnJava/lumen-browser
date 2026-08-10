# WPT vendor notes — `webrtc-identity`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webrtc-identity`, `docs/wpt-status.md`), scope 🚫 confirmed
exactly (checked before vendoring, same discipline as the `webrtc-ice`/
`webrtc-encoded-transform` siblings after the parent `webrtc`'s note turned
out inaccurate): `grep -n 'RTCIdentity|IdentityProvider|IdentityAssertion|
peerIdentity' crates/js/src/webrtc_stub.rs` finds zero matches — the shim's
`RTCPeerConnection` constructor stores `config` verbatim as `this._config`
without reading or validating a `peerIdentity` field at all, and no
`RTCIdentityProviderRegistrar`/`RTCIdentityAssertion`/identity-provider
plumbing exists anywhere in the crate.

Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from the sibling `webrtc-ice`, 5 files total
(4 test files + 1 `.sub.js` helper + `META.yml`), 4 glob-counted ids, no
`name="variant"` fan-out, zero `testdriver.js` hits, 3 of 4 test files are
`.https.`/`.sub.https.`.

`run_report.py --all --root webrtc-identity --recursive` (~57 s wall-clock):
**1/4 harness OK, 0/1 subtests passed**. All three `.https.` files
(`RTCPeerConnection-getIdentityAssertion.sub.https.html`,
`RTCPeerConnection-peerIdentity.https.html`, `idlharness.https.window.html`)
hit the TLS handshake directly (`network error: TLS handshake: invalid peer
certificate: UnknownIssuer`) — the already-documented TLS gap
[BUG-657](../../bugs/BUG-657-OPEN.md). The first of the three additionally
tripped the stale-browsing-context assertion
(`AssertionError: Got results from RTCPeerConnection-constructor.html,
expected RTCPeerConnection-getIdentityAssertion.sub.https.html`) — the same
session-reuse mechanism as [BUG-380](../../bugs/BUG-380-FIXED.md), triggered
here by the failed TLS navigation rather than a genuine second finding.

The one non-`.https.` file, `RTCPeerConnection-constructor.html`, ran to
completion and failed its single subtest: "RTCPeerConnection constructor
throws if the given peerIdentity getter throws" —
`assert_throws_js: function "() => new RTCPeerConnection({ peerIdentity:
toStringThrows })" did not throw`. This is an instance of the constructor
never validating `RTCConfiguration` fields, already filed as
[BUG-721](../../bugs/BUG-721-OPEN.md) (found on the parent `webrtc`
category via `iceServers`/`setConfiguration`/`getConfiguration`) — same root
cause (`webrtc_stub.rs` stores `config` verbatim, line ~126-145), not a
distinct defect worth its own number.

No new `BUG-NNN` filed.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 подтверждён точно до вендоринга: `grep` по
`RTCIdentity|IdentityProvider|IdentityAssertion|peerIdentity` в
`crates/js/src/webrtc_stub.rs` — ноль совпадений, конструктор
`RTCPeerConnection` сохраняет `config` целиком без чтения поля
`peerIdentity`, отдельного identity-provider API в крейте нет вовсе.
Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webrtc-identity/`,
5 файлов, 4 id по глобу, без variant-фан-аута, 0 `testdriver.js`).
`run_report.py --all --root webrtc-identity --recursive` — ~57 с,
**1/4 harness OK, 0/1 сабтестов**: три `.https.`-файла падают TIMEOUT/ERROR
на уже задокументированном TLS-гэпе [BUG-657](../bugs/BUG-657-OPEN.md)
(`UnknownIssuer`), один из них попутно словил BUG-380-паттерн (устаревшие
результаты предыдущего теста из-за неудавшейся навигации). Единственный
исполнившийся тест (`RTCPeerConnection-constructor.html`) падает на
отсутствии валидации `peerIdentity` в конструкторе — тот же корень, что уже
описан в [BUG-721](../bugs/BUG-721-OPEN.md) (конструктор не валидирует
`RTCConfiguration`). Новый BUG-NNN не заводился.
