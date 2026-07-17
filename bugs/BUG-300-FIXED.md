# BUG-300 — `browsingContext.navigate`'s `DocumentReady` wait can ACK using the *previous* page's stale `layout_box` when no JS context exists yet

**Статус:** FIXED 2026-07-17
**Компонент:** shell (`crates/shell/src/main.rs`, `check_wait_condition`)
**Найден:** P2-wpt S4, same diagnosis session as [BUG-298](BUG-298-FIXED.md)/[BUG-299](BUG-299-FIXED.md)

## Симптом

A BiDi client spawning `lumen --bidi-port <N>` and issuing `browsingContext.navigate(..., wait: "complete")` as its **first** navigation from the freshly-launched (blank) tab sometimes gets an ACK in ~10ms — far faster than the real page's fetch/parse/script pipeline could possibly have completed — and all subsequent `script.evaluate` calls against that context report stale/`undefined` state for the new page (reproduced driving `tools/wptrunner` end to end, `tests/wpt/run_smoke.py`).

## Причина

`check_wait_condition` (`crates/shell/src/main.rs`) for `WaitCondition::DocumentReady`/`NetworkIdle`:

```rust
match route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
    j.eval_js_value("document.readyState")
}) {
    Some(Ok(json)) => self.nav_start.is_none() && json == "\"complete\"",
    _ => self.layout_box.is_some(),   // <- no nav_start gate
}
```

The `Some(Ok(json))` branch (JS context present, `document.readyState` readable) correctly requires `self.nav_start.is_none()` — i.e. that the navigation that's actually being waited on has already installed its own fresh JS context (`apply_loaded_page` clears `nav_start` only after doing so; see the doc comment above this function, added by P2-wpt S1). The fallback branch — taken when no JS context exists to query yet, e.g. right at the start of a navigation before `apply_loaded_page` has run — has **no such gate**: it reports ready purely from `self.layout_box.is_some()`, which for a tab that has rendered *anything* before (including the initial blank tab's own empty layout) is `true` well before the new page has started loading. `AutomationCommand::Wait`'s per-frame poll (`about_to_wait`) can hit this fallback on the very first frame after `Navigate`+`Wait` are queued — before the navigation has even begun — and ACK immediately using the *old* state, the same "stale state wins the race" shape [BUG-296](BUG-296-FIXED.md) fixed for session restore.

## Фикс (2026-07-17)

Gate the fallback the same way as the real-readyState branch:

```rust
_ => self.nav_start.is_none() && self.layout_box.is_some(),
```

Preserves the fallback's purpose (don't hang forever waiting for a readiness signal that will never arrive on a JS-less tab/build) while preventing it from reporting ready during an in-flight navigation whose real signal hasn't arrived yet.

## Остаток

Reproducing `tests/wpt/run_smoke.py` after this fix (combined with [BUG-298](BUG-298-FIXED.md)/[BUG-299](BUG-299-FIXED.md)) still times out — but the symptom has changed and narrowed: `navigate()` now takes a realistic ~0.25–0.6s (previously ~10ms), and manual `script.evaluate` probes against the identical binary, driven directly over BiDi with a plain HTTP server for the same vendored test file, complete correctly (`window.__lumen_wpt_results` becomes a populated JSON string within ~2s). The remaining gap only reproduces when the test is served through the vendored `wptserve` and driven through the full `wptrunner`/`LumenTestharnessExecutor` path: `document.readyState` reads `"complete"`, all expected globals (`test`, `assert_true`, `add_completion_callback`) exist, the 3 expected `<script>` elements are present, but `document.getElementById('log')` — which `Output.prototype.show_status`/`show_results` create as their very first action — never appears at any point in a 15s window, meaning the harness's `completion` callback chain (or the `test_state`/`result` callbacks that fire earlier) never actually runs in this specific configuration, despite every externally-observable precondition looking identical to the working manual-probe case. Root cause not found — filed as [BUG-301](BUG-301-OPEN.md) for follow-up; not the same class of bug as BUG-298/299/300 (all three of those are proven fixed via direct, deterministic repro).
