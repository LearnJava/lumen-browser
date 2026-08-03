# BUG-523: `Element.scrollTop`/`scrollLeft` setter is queued and applied
asynchronously by the shell — a synchronous read right after the write sees
the stale (pre-write) value instead of the just-set position

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js/shell boundary (`crates/js/src/dom.rs:6196-6205` setters,
`crates/js/src/v8_runtime.rs:3103-3108` `_lumen_request_scroll`, shell's
`take_scroll_requests()` drain loop)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scroll-anchoring`

## Механизм

The JS shim's `scrollTop`/`scrollLeft` setters (`dom.rs:6199`/`6203`) don't
mutate any JS-visible state directly — they call the native
`_lumen_request_scroll(nid, x, y)`, which merely pushes `(nid, x, y)` onto a
`pending_scrolls: Arc<Mutex<Vec<...>>>` queue (`v8_runtime.rs:3106`). The
shell drains that queue on its own schedule (`take_scroll_requests()`,
frame/event-loop tied) and only *then* updates the `scroll_states` map that
the getter (`_lumen_get_scroll_state`, `dom.rs:6200`/`6196`) reads. Nothing
forces that drain to happen synchronously in response to the property write,
so a script that writes then immediately reads (in the same task, the CSSOM
View-mandated pattern used by virtually every scroll test) observes the
value from *before* the write, not the target it just set.

Confirmed live (`--mcp-live-port`, minimal isolation, two independent
scrollable `<div overflow:scroll>` elements, unrelated to
css-scroll-anchoring specifically):

```js
// same eval call, write then immediate read:
var e = document.getElementById('s');
e.scrollTop = 200;
e.scrollTop   // => 0 (stale)

// a few hundred ms later, a SEPARATE eval call on the same element:
document.getElementById('s').scrollTop   // => 200 (correct once the shell drained the queue)

// explicit timing: write, immediate read, then read again after 500ms wall-clock sleep
document.getElementById('t').scrollTop = 175; document.getElementById('t').scrollTop  // => 0
// ... 500ms later, separate eval call ...
document.getElementById('t').scrollTop   // => 175
```

This is the same architectural symptom family as
[BUG-493](BUG-493-OPEN.md) (script mutates state, then reads a
derived/cached value in the same task and gets the stale snapshot) but a
*different* code path/root cause — BUG-493 is about the `computed_styles`
cache populated by `update_computed_styles`, this is about the
`pending_scrolls`/`scroll_states` queue-and-publish pair. Filed separately
because the fix lives in different code (there's no single "force a
synchronous flush" call this shares with BUG-493's fix).

## Симптом

Any WPT test that does `el.scrollTop = N; assert_equals(el.scrollTop, N)` (or
the div/window equivalents) in the same script turn fails with `expected N
but got 0` (or whatever the previous scroll position was) — this is the
dominant failure cluster of `css/css-scroll-anchoring` (18+ files hit the
`Cannot set properties of undefined` variant when `document.scrollingElement`
compounds this — see [BUG-525](BUG-525-OPEN.md) — and 8+ hit the bare
`assert_equals: expected N but got 0` form on real element scrollers).
Likely affects any other WPT category whose tests script-drive scrolling and
assert synchronously (`css-overflow`, `cssom-view`, `css-scroll-snap` are
candidates worth re-checking once this lands).

## Фикс (не сделан)

Either (a) make the setter apply the scroll position to an in-memory
JS-visible cache synchronously (mirroring what real browsers do — the
*visual*/smooth animation can still be async, but the script-visible value
must update immediately per CSSOM View §scrolling), or (b) force a
synchronous drain-and-republish of `pending_scrolls`→`scroll_states` for the
specific `nid` inside the getter when a pending request for that node
exists (same shape of fix pattern the eventual BUG-493 fix will need for
`computed_styles`).
