# BUG-298 — `Element`/`DocumentFragment`/`ShadowRoot`.querySelector(All) search the whole document, not the calling node's subtree

**Статус:** FIXED 2026-07-17
**Компонент:** js (`crates/js/src/dom.rs` shim + `crates/js/src/v8_runtime.rs` native bindings, `crates/engine/layout/src/selector_query.rs`)
**Найден:** P2-wpt S4, re-diagnosing the "`script.evaluate` never resolves" symptom attributed to an environment-dependent JS-context-install race (`CLAUDE.md` → "Known gotchas")

## Симптом

`testharness.js`'s `Output.prototype.show_results` (the built-in results-table renderer, run as a `completion` callback) never produces a visible effect, and any code building a detached DOM subtree (create elements, `appendChild` them together, then `querySelector` within that subtree before attaching it to the document) gets `null`/empty results even though the queried element is genuinely present as a descendant.

## Причина

`Element.prototype.querySelector`/`querySelectorAll` (and the same methods on `DocumentFragment`/`ShadowRoot` wrappers) in the shared JS shim (`WEB_API_SHIM`, `crates/js/src/dom.rs`) delegated to the same native bindings `document.querySelector`/`querySelectorAll` use — `_lumen_query_selector`/`_lumen_query_selector_all` — which always call `lumen_layout::query_all(&doc, sel)`, a function that unconditionally starts its traversal at `doc.root()`. The calling node's own id (`nid`, available in every one of these closures) was never passed through, so these "scoped" methods were never actually scoped: they always searched the entire document.

For a **detached** subtree (an element created via `document.createElement`/`createElementNS` but not yet attached anywhere), the whole-document search can never find anything inside it, since the subtree isn't part of the document tree at all — `section.querySelector('table')` on a freshly-built `<section>` containing a manually-appended `<table>` returned `null` unconditionally.

`testharness.js`'s `Output.prototype.show_results` builds its entire results table this way (`render()` constructs a detached tree, then queries into it — e.g. `section.querySelector("tbody")`) before appending it to the live document. `tbody` came back `null`, so `tbody.appendChild(...)` threw `TypeError: Cannot read properties of null`. Because this exception is thrown from inside the *first* registered `completion` callback (`WindowTestEnvironment.on_tests_ready()` registers `output_handler.show_results` before `testharnessreport.js`'s own callback that reports results back to the harness), and `testharness.js`'s own callback dispatch loop (`forEach`) has no per-callback exception isolation, the throw aborted the whole completion sequence — the *harness's own* results-reporting callback (registered second) never ran. Combined with `crates/js/src/dom.rs`'s blanket `try { callback(); } catch(e) {}` pattern around essentially every native→JS callback dispatch (timers, events — no `window.onerror`, no `console.error`, nothing surfaced), the failure was completely silent: no exception anywhere, no diagnostic trace, `document.readyState` reads `"complete"`, all expected globals exist — the only symptom was that the harness's completion signal (`window.__lumen_wpt_results`) never appeared, indistinguishable from the previously-suspected JS-context-install race described in `CLAUDE.md`.

Confirmed with a minimal repro (bisecting each DOM call `Output.prototype.show_results` makes): `document.createElementNS(...)` + `appendChild` work fine; `detachedSection.querySelector('table')` after appending a `<table>` child returns `FAIL` (`null`) unconditionally, independent of environment/timing — fully deterministic, not a race.

## Фикс (2026-07-17)

- `crates/engine/layout/src/selector_query.rs`: new `query_all_within(doc, start, sel) -> Vec<NodeId>` — same matcher as `query_all`, but traverses only `start`'s descendants (excluding `start` itself), matching `Element`/`DocumentFragment`/`ShadowRoot` `querySelector(All)` scoping per DOM LS §4.2.6. Re-exported from `lumen_layout`.
- New native bindings `_lumen_query_selector_scoped(node_id, sel)` / `_lumen_query_selector_all_scoped(node_id, sel)`, registered identically in both `crates/js/src/v8_runtime.rs` (V8, default engine) and `crates/js/src/dom.rs` (QuickJS, rollback path) — mirroring the existing unscoped bindings' registration pattern.
- `WEB_API_SHIM` (`crates/js/src/dom.rs`, shared JS text): `Element.prototype.querySelector/querySelectorAll`, the `ShadowRoot` wrapper, and the `DocumentFragment` wrapper now call the scoped bindings with `nid`. `document.querySelector/querySelectorAll` (genuinely document-wide by spec) and `getElementsByTagName` are unchanged.

Verified directly against a live `lumen --bidi-port` window: a detached `<section>` with an appended `<table id="results">` now finds it via `section.querySelector('table')`.
