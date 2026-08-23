# BUG-661 — `ResizeObserver` shim: no guaranteed initial delivery, no argument validation, no border/content/scrollbar box distinction, reparenting doesn't reset observation state

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7085`–`7138` — `ResizeObserver`/`_lumen_deliver_resize_observers`), shell (`crates/shell/src/main.rs:10300` — `deliver_layout_observers()` call site, gated on relayout)
**Найден:** P2, WPT-VENDOR-resize-observer (2026-08-05), `run_report.py --all --root resize-observer --recursive` real run

## Live run signal

```
tests: 3/15 harness OK; subtests: 13/32 passed
```

12 of 15 files TIMEOUT at the harness level; several of those TIMEOUTs (`ordering.html`,
`calculate-depth-for-node.html`) have **zero subtest output at all** — the callback never
fires even once. Four distinct, independently-confirmed root causes:

## 1. Delivery is entirely piggybacked on the relayout pipeline — no guaranteed delivery on the next turn after `observe()`

Per spec (Resize Observer §13.4, "process resize observations"), the observation loop runs
unconditionally as part of the update-the-rendering steps; a newly `observe()`d target with
no previously-reported size is *always* reported on the very next pass, independent of
whether anything else changed. Lumen instead only calls `_lumen_deliver_resize_observers()`
from inside the relayout path (`main.rs:10300`, `deliver_layout_observers()`), which itself
only runs when the shell decides a relayout is needed (dirty DOM/style). Confirmed live via
`--mcp-live-port`: `document.documentElement` given no children beyond ordinary page content,
`ro.observe(document.documentElement)` called from `eval()` — polled every 300ms for 3s,
zero deliveries, even though `_lumen_deliver_resize_observers()`'s own per-observation check
(`o.lastW = -1` initially, so any real width should exceed the `< 0.5` epsilon) would have
fired if it had ever been invoked. Only after an unrelated `document.documentElement.style.fontSize = '20px'`
mutation (forcing a real relayout) did the observer fire — one delivery, for the *original*
un-notified observation plus the new size, collapsed into a single entry.

This explains the pure-TIMEOUT-no-subtests cases: `ordering.html` calls `.observe()` on
`document.documentElement` from the initial parse-time script with no further mutation
anywhere in the test, expecting the guaranteed initial notification; `calculate-depth-for-node.html`
observes an empty, unstyled `<div>` the same way. Neither test ever mutates anything after
the initial `observe()` call, so under the current architecture no relayout is ever scheduled
and the callback never runs.

## 2. `observe()` performs no argument validation — silently no-ops instead of throwing

```js
ResizeObserver.prototype.observe = function(target) {
    if (!target || target.__nid__ === undefined) return;
    ...
};
```
(`dom.rs:7096`–`7102`) Spec requires `observe(target)` to throw a `TypeError` when `target`
is not an `Element`. Lumen's shim just returns silently for any non-Element/`undefined`
argument. `observe.html` — `test2: throw exception when observing non-element` —
`assert_throws_js` fails with "did not throw".

## 3. `contentRect`/`borderBoxSize`/`contentBoxSize`/`devicePixelContentBoxSize` are all populated from one flat rect — no box-model distinction, no scrollbar subtraction

```js
entries.push({
    target: o.target,
    contentRect: { x: rect[0], y: rect[1], width: w, height: h, ... },
    borderBoxSize:  [{ inlineSize: w, blockSize: h }],
    contentBoxSize: [{ inlineSize: w, blockSize: h }],
    devicePixelContentBoxSize: [{ inlineSize: w, blockSize: h }],
});
```
(`dom.rs:7125`–`7132`) All four size representations get the exact same `w`/`h` from
`_lumen_get_bounding_rect`. Per spec these must differ: content-box excludes padding,
border and (per CSSOM View, scrollbar gutters); border-box includes padding+border;
device-pixel-content-box is the content box scaled by device pixel ratio and rounded.
`scrollbars.html` — "ResizeObserver content-box size and scrollbars" —
`assert_equals: expected 2 but got 1` (the content-box width isn't shrunk by the scrollbar
track at all).

## 4. Reparenting (detach + reattach) doesn't reset the observation's last-reported size

```js
this._observations.push({ target: target, lastW: -1, lastH: -1 });
...
if (Math.abs(w - o.lastW) < 0.5 && Math.abs(h - o.lastH) < 0.5) continue;
```
`lastW`/`lastH` are only ever reset by `unobserve()`/`disconnect()` — a `target.remove()`
followed by `parent.appendChild(target)` at the *same* size leaves `lastW`/`lastH` unchanged,
so no entry is produced. Per spec, detaching an observed element (removing it from the
document) is itself an observable size change (the box no longer participates in layout),
so reattachment at the same visual size must still deliver a fresh notification.
`notify.html` — `test2: remove/appendChild trigger notification` —
`assert_unreached: Timed out waiting for notification. (1000ms)`.

## Что НЕ является причиной этого бага (уже задокументированные/отдельные гэпы)

- `idlharness.window.html` — TIMEOUT; recurring un-vendored `WebIDLParser.js`/`idlharness.js`
  infra gap seen across every idlharness category so far.
- `svg-with-css-box-001.html`/`svg.html` — TIMEOUT/FAIL on `foreignObject`/SVG size
  observation; not investigated further this session (needs its own SVG-geometry
  root-cause pass, likely compounded by finding 1 above since these tests also rely on the
  guaranteed-initial-delivery behavior).
- `fragments.html` — CSS multicol fragment-aware `contentBoxSize`/`borderBoxSize` arrays
  (one entry per fragment) aren't produced at all (the shim always pushes a single-element
  array) — real gap, but scoped to multicol fragmentation support, not filed as part of this
  bug's four findings.
- `callback-cross-realm-report-exception.html`, `zoom.html`, `eventloop.html` — pure
  TIMEOUT, not traced to a specific line this session; plausibly downstream of finding 1
  (no guaranteed delivery) compounding with cross-realm/zoom-specific logic, worth
  re-triaging after finding 1 is fixed rather than guessing further now.

## Предлагаемый фикс

Finding 1 is the highest-value fix: make `_lumen_deliver_resize_observers()` (or an
equivalent pass) run at least once per animation-frame/task-queue turn whenever any
observer has at least one observation with `lastW === -1` (never yet reported), independent
of whether the shell's relayout-dirty check would otherwise skip a pass — the cheapest way
is likely to treat "at least one un-reported ResizeObservation exists" as its own dirty-flag
input to the relayout scheduler. Findings 2–4 are small, independent, single-function fixes
(`observe()` type check + throw; splitting `_lumen_get_bounding_rect`'s single rect into
distinct border/content/device-pixel geometries with scrollbar-gutter subtraction; resetting
`lastW`/`lastH` to `-1` when a `MutationObserver`-visible detach is seen for an observed
target) and can land independently of finding 1.

## Срез 24 WPT-RUN-6 (2026-08-22) — finding 1 перезамерен и получил маркер

`tests/wpt/verify_frame_load_media_gaps.py --variant ro-basic` (dev-release,
Linux, коммит `c583a90b4`, `--seconds 5`, страница жива — 9 тиков): на
статическом элементе `observe()` не приводит к вызову колбэка вовсе; первый и
единственный `ro-callback n=1 h=120` приходит только после того, как проба
сама меняет высоту (`ro-resized`). То есть «нет гарантированной первой
доставки» — это не «доставка с задержкой», а её отсутствие: страница, которая
ничего не меняет, не получает ни одного колбэка.

Маркер `resize-observer-no-initial` в `tests/wpt/timeout_audit.py` — **4 id**
остатка снимка WPT-RUN-5: `css/css-sizing/contain-intrinsic-size/auto-001`,
`-002`, `-004`, `-005`. Все четыре построены одинаково — весь текст теста
живёт внутри колбэка `ResizeObserver`, поставленного на неизменный элемент,
поэтому подтест уходит в TIMEOUT, не выполнив ни одной проверки. Соседние id
того же каталога висят на отсутствии `contentvisibilityautostatechange` —
[BUG-852](BUG-852-OPEN.md).
