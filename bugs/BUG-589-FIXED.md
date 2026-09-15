# BUG-589: `window` is not a proper WebIDL exotic object — no `Symbol.toStringTag`, indexed `[[DefineOwnProperty]]`/`[[Set]]` don't reject out-of-range numeric keys

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js/V8 host-object layer (`crates/js/src/v8_runtime/named_access.rs`, `crates/js/src/shim/web_api_shim_tail_b.js`, `crates/js/src/frame_bridge.rs`)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL window object - assert_class_string: expected "[object Window]" but got "[object Object]"
FAIL Global scope polluter - assert_class_string: expected "[object WindowProperties]" but got "[object Object]"
```
(`html/browsers/the-window-object/window-prototype-chain.html`, self-contained,
no iframe dependency)

```
FAIL Borderline numeric key: 2 ** 32 - 2 is an index (strict mode) - assert_throws_js: function "() => { window[4294967294] = 1; }" did not throw
```
(`html/browsers/the-window-object/window-indexed-properties-strict.html`,
this specific assertion doesn't depend on any iframe existing)

## Причина

Two independent gaps in `window`'s exotic-object behavior:

1. Neither `window` nor the "WindowProperties" object in its prototype chain
   (`Object.getPrototypeOf(Object.getPrototypeOf(window))`) carry a
   `Symbol.toStringTag`, so `Object.prototype.toString.call(window)` answers
   the generic `[object Object]` instead of `[object Window]` — the same
   class of defect already tracked for `Headers`/`Response`/
   `CredentialsContainer`/etc. in [BUG-369](BUG-369-FIXED.md)/
   [BUG-366](BUG-366-FIXED.md), here on the global object itself.
2. `window`'s indexed-property `[[DefineOwnProperty]]`/`[[Set]]` don't
   implement the WebIDL "index in `[0, 2**32-2]` with no indexed setter for
   that slot → throw `TypeError` in strict mode" rule at all: any numeric
   key silently accepts a plain assignment instead of being rejected. This
   holds even for indices that can never correspond to a real nested
   browsing context (`2**32-2`), so it's independent of the iframe-support
   limitation.

## Масштаб

Both assertions above are self-contained (no iframe required). The rest of
`window-indexed-properties-strict.html`/`window-indexed-properties.html`/
`named-access-on-the-window-object/window-named-properties.html` layers the
already-known "`<iframe>` without browsing context" limitation on top (they
assert `window[0] === iframe.contentWindow`), so expect those to need both
fixes before going green.

## Исправление

Живой пробой на a real `V8JsRuntime` (`install_dom`, not just reading the
source) showed the gap was wider than the two symptoms above suggest:
`typeof Window === 'undefined'` — no `Window` global constructor existed at
all — and `window instanceof EventTarget === false`, because `window`'s
prototype chain was V8's own internal global-proxy/global-object plumbing,
with no HTML-specific link in it whatsoever (own methods like
`addEventListener` made `window` *behave* like an `EventTarget` without the
identity ever showing up in the chain).

**`Symbol.toStringTag`/prototype chain** (`web_api_shim_tail_b.js`, in the
`window = globalThis` bootstrap block): built a `Window` function with
`Window.prototype` tagged `'Window'`, an intermediate "global scope
polluter" object (`Object.create(EventTarget.prototype)`) tagged
`'WindowProperties'`, and `Object.setPrototypeOf(window, Window.prototype)`
— the exact same move `worker_location_navigator_shim.js` already uses for
`WorkerGlobalScope` in worker contexts. Own properties shadow the prototype
chain, so nothing observable changed except `instanceof`/`toString`/
`getPrototypeOf` — confirmed by a dedicated regression test that dispatches
a real event through `window.addEventListener` after the prototype swap.

**Indexed `[[DefineOwnProperty]]`/`[[Set]]`** (`named_access.rs`): added an
`IndexedPropertyHandlerConfiguration` to the global `ObjectTemplate` with a
`definer`/`setter` that unconditionally report failure. This matches the
WebIDL algorithm precisely: `window` has no indexed property *setter*
(only a getter), so §Legacy platform objects' `[[DefineOwnProperty]]` says
every array-index define fails, whether or not the index happens to be a
live nested browsing context — `window[0] = "foo"` must throw exactly as
hard as `window[999999] = "foo"` even when index 0 is a real iframe.

That created a real conflict with existing code: `_lumen_frame_install_index`
(`frame_bridge.rs`) — the mechanism that installs the actual `window[i]`/
`window[name]` getters when a frame registers — uses `Object.defineProperty`
internally, and the new handler started rejecting that too (silently, inside
its own `try {} catch (e) {}`), breaking `window[0]`/`window.length` for
real frames (caught by `frame_bridge::tests::
frame_length_and_accessors_track_registrations`). Fixed with a narrow,
JS-level trust flag: `_lumen_frame_install_index` raises
`globalThis._lumen_indexed_define_trusted = true` only around its own
`Object.defineProperty` call, and the definer/setter (`indexed_define_trusted`
helper) decline to intercept while it's set — page script has no way to set
that flag meaningfully (it is read fresh on every call and is not itself an
indexed key, so it never round-trips through the handler it gates).

5 new tests, `dom::tests::v8_bug589_window_exotic_object` (`crates/js/src/dom/tests/`):
reports as `[object Window]` with the right prototype identity, the GSP
object tagged and chained through `EventTarget.prototype`/`Object.prototype`,
`window` still a working `EventTarget`, the strict-mode/`Reflect` rejection
matrix for an unsupported index, and the `2**32-1`/`-1` boundary keys staying
unaffected. `cargo test -p lumen-js --lib --features v8-backend` 3682/3682
(was 3677), `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean. Full `scoped-test.sh`/`cargo clippy --workspace` not
completed — the closure pulls in `lumen-network`, whose gate hangs
independently ([BUG-805](BUG-805-OPEN.md)); the affected crate was tested
directly and is green.
