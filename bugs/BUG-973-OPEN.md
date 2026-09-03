# BUG-973 — `history.back()`/`forward()`/`go(n)` resolve their target against
the nav stack at *drain* time, not at call time

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/lumen/navigation.rs::navigate_by`,
fed by `crates/shell/src/app/about_to_wait.rs`'s `take_history_traversals`
drain) / js (`crates/js/src/v8_runtime/install/storage.rs::_lumen_history_go`
→ `_lumen_history_traverse`, which only ever queues a bare relative `delta`)
**Найден:** P2, WPT-RUN-6 срез 56, живой пробой, while verifying the
[BUG-971](BUG-971-FIXED.md) fix

## Механизм

`history.go(delta)` (and `back`/`forward`, both `go(±1)`) does two things on
the JS side: it moves the JS-local `HistoryState` read-cache immediately,
and it pushes the bare integer `delta` onto `pending_history_traversals`
(`Vec<i32>`, `storage.rs:51`). The shell drains that queue once per
event-loop tick (`about_to_wait.rs:574`) and calls `self.navigate_by(delta)`
(`navigation.rs:621`), which shifts entries between the live `nav_back`/
`nav_fwd` stacks **as they stand at drain time**. Nothing captures which
entry was current *when `history.back()` was actually called* — only the
signed step count survives the trip.

HTML LS's traversal algorithm instead resolves the destination *synchronously*
at call time (`history.go` computes `targetStep = current entry's step +
delta` before returning; the deferred task only performs the navigation to
that already-fixed entry). Lumen's deferred-delta design collapses those two
steps into one that runs entirely at drain time, so a delta computed for one
stack shape gets applied to a different one if the stack changes in between.

## Прямое измерение

Two same-document navigations racing inside one synchronous turn (the exact
shape `anchor-fragment-history-back-on-click.html` uses), minimal page, no
`testharness.js` (`.tmp/probe56_anchor_fragment.html`, not committed;
dev-release, Linux, `main` after the BUG-971 fix): entries are pushed
`#1 → #2` (script), then inside a synchronous `<a>` `onclick` handler
`history.back()` is called (stack is at `#2`, so the naive/spec-correct
target is `#1`), and only *after* the handler returns does that same click's
default activation push `#3` (stack is now at `#3`). By the time the queued
traversal drains, `nav_back`/`nav_fwd` reflect `#3` as current, and
`delta = -1` lands on `#2` — one entry short of the `#1` the call actually
meant. Observed: `navigations=["#3","#2"]`, `hash=#2` at the end, instead of
the spec/real-browser-expected `["#3","#1"]`.

## Кого это держит

Real trigger:
`html/browsers/browsing-the-web/overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`.
With BUG-971 fixed this id no longer hangs (`harness-complete status=0`),
but its sole subtest now FAILs promptly instead of TIMING OUT — a real
progress, not a full pass. Not reclassified in `timeout_audit.py` (no
TIMEOUT left to explain); noted here because BUG-971's fix is what exposed
it cleanly.

Any page that calls `history.back()`/`forward()`/`go()` from inside a
synchronous handler that itself triggers another same-document navigation
before the event loop's next tick (a fairly ordinary "undo the last hash
change, but the click that triggered undo also navigates" pattern) gets a
traversal target off by however many entries were inserted in between.

## Направление починки (не предписание)

`_lumen_history_go`/`_lumen_history_traverse` need to resolve and queue an
*absolute* target (an entry identity or session-history step index) at call
time, using the JS-side `HistoryState` cache's position *before* any later
same-document push can move it — not a bare relative delta consumed however
many ticks later. `navigate_by` then travels to that fixed target rather than
re-deriving one from whatever `nav_back`/`nav_fwd` look like when it runs.
