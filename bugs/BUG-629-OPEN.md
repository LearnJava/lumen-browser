# BUG-629: Reporting API shim — `ReportingObserver`/`Report` callable without `new` (pollutes `globalThis`), `Report` is illegally constructible, prototype methods wrongly enumerable

**Статус:** OPEN
**Компонент:** js (`crates/js/src/reporting_api.rs`, `REPORTING_API_SHIM`)
**Найден:** P2, WPT-VENDOR-intervention-reporting, 2026-08-05

## Симптом

`intervention-reporting`'s only test (`idlharness.any.js`) TIMEOUTs on the
known unvendored `/resources/WebIDLParser.js`+`idlharness.js` gap (0/1
harness OK), so the category itself carries no signal — but per the
"probe the container one level up" rule ([[reference_wpt_run_report_invocation_recipe]]),
the API it targets (Reporting API, shared with the in-scope `reporting`
category) is implemented in `crates/js/src/reporting_api.rs` and worth
probing directly. Live probe via `--mcp-live-port` on a plain page found
three independent WebIDL-conformance defects, all confirmed:

**1. `ReportingObserver` called without `new` silently succeeds and
pollutes `globalThis`, instead of throwing.**

```js
var o = ReportingObserver(function(){});
// → o === undefined, no throw

o === globalThis            // false (o is undefined, the function has no explicit return)
globalThis._callback        // "function" — the observer's internal state now lives on window
globalThis._types, globalThis._buffered, globalThis._queue, globalThis._observing  // all leaked too
```

Per WebIDL, an interface's constructor operation MUST throw a `TypeError`
when invoked without `new` (`Failed to construct 'ReportingObserver':
Please use the 'new' operator, not 'call'`). Here `ReportingObserver` is a
plain `function ReportingObserver(callback, opts) { this._callback = ...; }`
— called without `new` in non-strict-mode shim code, `this` binds to
`globalThis`, so every field the constructor sets becomes a same-named
global property. `_callback`/`_types`/`_queue` etc. are generic enough
names that any other library or page script relying on them is silently
clobbered.

**2. `Report` has the same defect, and the polluted names are far more
dangerous — `type` and `url` are two of the most common global
identifiers on the web.**

```js
var x = Report("t", "u", null);
// → x === globalThis is false, but:
globalThis.type   // "t"
globalThis.url    // "u"
```

Any page or script that has `var type` / `var url` in global scope (or
reads `window.type`/`window.url`) is silently corrupted by an unrelated
call. Confirmed via `--mcp-live-port` eval on a minimal test page.

**3. `Report` itself is illegally constructible even *with* `new` — the
spec defines no constructor for it at all.**

```js
new Report("csp-violation", "https://evil.example/", {fake: true}) instanceof Report
// → true
```

Per the W3C Reporting API (`interface Report { readonly attribute
DOMString type; readonly attribute USVString url; readonly attribute
object? body; };`), `Report` instances are meant to be created only
internally by the browser (delivered through `ReportingObserver`
callbacks / the `report-to` mechanism) — there is no `constructor()`
operation in the IDL, so `new Report(...)` from page script should throw
`TypeError: Illegal constructor`. Exposing a plain constructable function
lets any script fabricate arbitrary `Report` objects (fake `type`/`url`/
`body`, indistinguishable via `instanceof Report` from a genuine one),
undermining any application code that trusts `report instanceof Report`
as a signal that the report actually originated from the browser.

**4. `ReportingObserver.prototype`'s methods are enumerable (should not
be).**

```js
Object.getOwnPropertyDescriptor(ReportingObserver.prototype, "observe")
// → {configurable: true, enumerable: true, writable: true, value: {}}
```

Per WebIDL, operations on an interface prototype object must be
non-enumerable (`{writable: true, enumerable: false, configurable:
true}`). Here they're plain `Ctor.prototype.method = function(){}`
assignments, which default to enumerable — so `for...in` over any
`ReportingObserver` instance walks the prototype chain and yields
`observe`, `disconnect`, `takeRecords`, and the internal helper
`_accepts` (an underscore-prefixed method clearly meant as private, but
fully web-visible — same class of leak as [[BUG-615]]'s enumerable
`_userState`/`_screenState`/etc. on `IdleDetector`, here on a prototype
instead of an instance).

## Причина

`REPORTING_API_SHIM` in `crates/js/src/reporting_api.rs` implements both
`Report` and `ReportingObserver` as classic ES5-style
`function Ctor(...) { this.x = ...; }` + `Ctor.prototype.method = ...`
declarations with no `new.target` guard and no
`Object.defineProperty(..., {enumerable: false})` on the prototype
methods — the same pattern already flagged for `IdleDetector` in
[[BUG-615]] (private state as enumerable instance properties) and for
other `EventTarget`-style shims per that bug's note ("same pattern `this._x`
seen in 7 other `EventTarget` shims"). This file was never covered by
that audit since it's not an `EventTarget` subclass, but the underlying
defect class (JS shim skips ES6 `class`/WebIDL-conformance semantics) is
identical.

## Масштаб

All three findings are live and reproducible on any page, independent of
WPT — the `intervention-reporting` category itself contributed no direct
signal (single test, unvendored-infra TIMEOUT). Finding 3 (forgeable
`Report`) is the one with actual security relevance: script-injected fake
`Report` objects can spoof a browser-originated intervention/deprecation/
CSP-violation report to any code trusting `instanceof Report`. Findings 1
and 2 (missing `new`-guard) are a correctness/robustness gap that a
`class`-based rewrite of both constructors (matching the fix pattern
[[BUG-615]] should use) would close simultaneously with finding 4.
