# BUG-779 — Worklet module loading is a no-op everywhere: `addModule()` never fetches/executes, and `CSS.animationWorklet`/`CSS.layoutWorklet`/`window.Worklet` don't exist at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/paint_worklet.rs:91-99` — `CSS.paintWorklet.addModule()`; `crates/js/src/web_audio.rs:485-487` — `audioContext.audioWorklet.addModule()`; no file implements `CSS.animationWorklet`/`CSS.layoutWorklet`/a generic `Worklet` constructor — confirmed absent by grep across `crates/js/src/*.rs` and a live probe)
**Найден:** P2, WPT-VENDOR-worklets, 2026-08-18 — grep + live `--mcp-live-port` probe (the `run_report.py --all --root worklets --recursive` run itself gives no signal here, masked by the pre-existing TLS `UnknownIssuer` gap, see "Почему находка не видна в прогоне" below)

## Симптом

The `worklets` WPT category (`tests/wpt/worklets/`, 21 `.https.html` files
covering `animation-worklet-*`/`audio-worklet-*`/`layout-worklet-*`/
`paint-worklet-*` × `credentials`/`csp`/`import`/`referrer`/
`service-worker-interception`) all exercise the same shared helper
(`resources/worklet-test-utils.js::get_worklet(type)`):

```js
function get_worklet(type) {
  if (type == 'animation') return CSS.animationWorklet;
  if (type == 'layout')    return CSS.layoutWorklet;
  if (type == 'paint')     return CSS.paintWorklet;
  if (type == 'audio')     return new OfflineAudioContext(2,44100*40,44100).audioWorklet;
}
```

A live probe (`--mcp-live-port`, plain `http://` page — the `.https.` WPT
pages never even reach this code, see below) shows:

```json
{"css_defined":"object","paintWorklet":"object","paintWorklet_addModule":"function",
 "animationWorklet":"undefined","layoutWorklet":"undefined",
 "audioContext_worklet":"object","window_Worklet":"undefined","registerPaint":"function"}
```

- `CSS.animationWorklet` and `CSS.layoutWorklet` are `undefined` —
  `get_worklet('animation')`/`get_worklet('layout')` return `undefined`, so
  any test/page calling `.addModule(...)` on the result throws
  `TypeError: Cannot read properties of undefined (reading 'addModule')`
  synchronously, before any network activity.
- `window.Worklet` (the abstract base interface every worklet type is
  supposed to implement, per the `worklets/idlharness.https.any.js` id) is
  `undefined` too — there is no generic `Worklet` constructor anywhere in
  the engine, only two ad-hoc namespaced objects.
- `CSS.paintWorklet` and `audioContext.audioWorklet` **do** exist, but
  their `addModule()` is a pure no-op that always resolves without ever
  fetching or executing the given URL:

  ```rust
  // crates/js/src/paint_worklet.rs:91-99
  addModule: function(moduleUrl) {
    return Promise.resolve().then(() => {
      // Phase 0 stub: accept the URL but don't fetch/execute it.
      this._currentModule = moduleUrl;
      return undefined;
    });
  }
  ```
  ```rust
  // crates/js/src/web_audio.rs:485-487
  this.audioWorklet = {
    addModule: function(url) { return Promise.resolve(); }
  };
  ```

  A page that calls `CSS.paintWorklet.addModule('my-paint.js')` gets a
  resolved promise — indistinguishable from a real, successful load — but
  the module is never fetched, so `registerPaint()` (which the module
  would normally call) never runs and the custom paint class never
  registers. `registerPaint` itself is real and works when called
  manually (verified by `paint_worklet.rs`'s own unit tests), but nothing
  in the current pipeline connects `addModule()` to actually running the
  module — so the *only* way to reach `registerPaint` today is to call it
  directly from the page's main-thread script, which isn't how any real
  Paint-Worklet-using site is written.

## Почему находка не видна в прогоне

Every file in the `worklets` category is `.https.`, and this machine's
WPT TLS setup fails every HTTPS navigation before the page's own JS ever
runs: `network error: TLS handshake: invalid peer certificate:
UnknownIssuer` → harness-level `TIMEOUT` (already the dominant pattern
across `webtransport`/`webusb`/`webxr`/`window-management`, same
documented gap tracked elsewhere in `docs/wpt-status.md`, not re-filed
here). `run_report.py --all --root worklets --recursive`: **0/21 harness
OK, 0/0 subtests** — a uniform TIMEOUT wall with no category-specific
signal. The defect above was found only by probing the API surface
directly (`--mcp-live-port`, per the "probe even when TLS masks
everything" pattern already used for `eyedropper`/`fenced-frame`), not by
reading the run's own output.

## Предлагаемый фикс

Not attempted this session (Houdini worklets are Phase 0 per
`paint_worklet.rs`'s own doc comment — "In Phase 1, this would fetch the
module, execute it in a worker context, and collect `registerPaint()`
calls via a proxy"). Filed to make the gap explicit and discoverable
rather than silently absent from `CAPABILITIES.md` (which does not
mention Houdini/worklets at all, so there is no claims-vs-reality drift —
this is a straightforward "not yet built" entry, not a regression).

## Не расследовано в этой сессии

- Whether `CSS.layoutWorklet` (CSS Layout API) or `CSS.animationWorklet`
  (Web Animations API worklet integration) are planned for any near-term
  phase — not found in `ROADMAP.md`/`docs/plan/*`.
- `idlharness.https.any.js`'s actual assertions were not run (blocked by
  both the TLS gap and the unvendored `WebIDLParser.js`/`idlharness.js`
  helpers, same documented gap as every other idlharness-based category).
