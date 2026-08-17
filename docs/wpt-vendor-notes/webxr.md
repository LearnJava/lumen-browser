# WPT vendor notes — `webxr`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webxr`, `docs/wpt-status.md`), scope 🚫 ("XR — no runtime",
pre-classified in `ROADMAP.md`). Same pinned commit `35be3b44`, `git
sparse-checkout add` at the same commit hash, `LICENSE-WPT.md` copied from a
sibling category, 178 files (165 glob ids, no variant fan-out).

Confirmed the scope call — `crates/js/src/webxr.rs` is a whole-file Phase-0
JS shim (`WEBXR_SHIM`). `navigator.xr` is a real `XRSystem` singleton
(extends `EventTarget`), `isSessionSupported()` always resolves `false`,
`requestSession()` always rejects `NotSupportedError`. `XRSession`/
`XRFrame`/`XRReferenceSpace`/`XRView` exist as classes with stub methods
(`getViewerPose()` → `null`, `requestAnimationFrame()` → `0`, etc.) but no
Rust-side XR runtime exists anywhere in the workspace to drive them. Scope
call stands: 🚫, a Phase-0-only surface with no reachable success path.

Predictor check: only 3 of 178 files pull `testdriver.js` against 164
`.https.` files — the testdriver-share predictor is subsumed here (per the
general rule already on record), so the run pays the full TLS-gap TIMEOUT
wall instead of a cheap SKIP wall.

`run_report.py --all --root webxr --recursive` (~17 min, venv python):
**1/165 harness OK, 17/19 subtests**. 164 of 165 ids are `.https.` (WebXR
requires a secure context by spec) and hit the pre-existing, already-documented
TLS-trust gap from WPT-RUN-2 (`tests/wpt/certs/README.md`, `UnknownIssuer`,
also tracked as [BUG-657](../../bugs/BUG-657-OPEN.md)) before touching
`webxr.rs` at all — not a category finding. Once the one non-`.https.` id
below became the last successfully-loaded document, every subsequent
`.https.` navigate attempt was reported by the (already-fixed, [BUG-380]
(../../bugs/BUG-380-FIXED.md)) staleness-marker check as `ERROR:
browsingContext.navigate(...) reported success but the document was never
replaced` instead of a raw harness TIMEOUT — this is the BUG-380 fix's
detector correctly re-surfacing the underlying [BUG-438]
(../../bugs/BUG-438-OPEN.md) defect (a failed load silently keeps the old
document while `navigate` still reports success), not a new or category-
specific hang.

The 2 non-`.https.` ids that did run:

- `historical.html`: **17/17 harness OK.** Purely checks that legacy WebVR
  interfaces (`VRDisplay` and friends, `navigator.getVRDisplays`,
  `onvrdisplay*` window handlers) are absent — trivially true, since Lumen
  never implemented WebVR at all. Not a signal about the current WebXR
  surface, just confirms the deprecated API was never added.
- `webxr_availability.http.sub.html`: harness **TIMEOUT, 0/2 subtests**.
  - Sync subtest (`Test webxr not available in insecure context`) FAILs:
    `forEachWebxrObject` asserts every WebXR global is `undefined` on an
    `http://` origin; `navigator.xr` is defined because `WEBXR_SHIM`
    installs it via unconditional `Object.defineProperty(navigator, 'xr',
    {...})` (`crates/js/src/webxr.rs:110-115`), with no `isSecureContext`
    branch, and `install_v8!(webxr::install_webxr_bindings_v8)`
    (`v8_runtime.rs:5167`) runs unconditionally too. Reconfirms the
    already-open umbrella bug [BUG-765](../../bugs/BUG-765-OPEN.md) ("no
    `[SecureContext]`-tagged API is gated by `window.isSecureContext`") —
    not a new number.
  - Async subtest (`Test webxr not available in secure context in insecure
    context`) times out: it appends a cross-origin `https://` `<iframe>` and
    waits for a `message` event from `frame.contentWindow`, which never
    fires because the iframe never gets a separate browsing context.
    Reconfirms the already-open [BUG-480](../../bugs/BUG-480-OPEN.md)
    (`<iframe>` has no browsing context) — not a new number.

No new `BUG-NNN` filed.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «XR — нет рантайма» подтверждён — `crates/js/src/webxr.rs` целиком
Phase-0: `navigator.xr` реален (`XRSystem`), но `isSessionSupported()` всегда
`false`, `requestSession()` всегда реджектит `NotSupportedError`; Rust-бэкенда
XR в воркспейсе нет. Вендорена целиком 2026-08-18 (коммит `35be3b44`,
`tests/wpt/webxr/`, 178 файлов, 165 id, без variant-фан-аута). `run_report.py
--all --root webxr --recursive` (~17 мин) — **1/165 harness OK, 17/19
сабтестов**. 164/165 — `.https.`-гэп TLS `UnknownIssuer`
([BUG-657](../bugs/BUG-657-OPEN.md)), не находка категории; после первой (и
единственной) успешной загрузки страницы все последующие навигации
отчитывались уже починенным ([BUG-380](../bugs/BUG-380-FIXED.md)) детектором
маркера как явный `ERROR` («document was never replaced») вместо голого
TIMEOUT — это переподтверждение движкового [BUG-438](../bugs/BUG-438-OPEN.md)
(навигация молча не грузит и рапортует успех), не новый/специфичный для
категории зависон. Два исполнившихся non-`.https.` теста: `historical.html`
17/17 OK (тривиально — проверяет отсутствие устаревших WebVR-интерфейсов,
которых никогда не было); `webxr_availability.http.sub.html` — harness
TIMEOUT, 0/2 сабтестов, переподтверждает [BUG-765](../bugs/BUG-765-OPEN.md)
(`navigator.xr` ставится безусловно, без гейта `isSecureContext`) и
[BUG-480](../bugs/BUG-480-OPEN.md) (кросс-origin `<iframe>` без browsing
context, второй сабтест виснет). Новый BUG-NNN не заводился.
