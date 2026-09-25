# BUG-534: CSS Custom Highlight API — `Highlight`/`HighlightRegistry` are ad-hoc Phase-0 stubs, not the spec's Setlike/Maplike interfaces

**Статус:** FIXED 2026-09-26 (P1, GAP-HLHITTEST)
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/highlight_api.rs` — `HIGHLIGHT_API_SHIM`, installed via `install_highlight_api_bindings_v8`)
**Найден:** P2, WPT-RUN-3 срез 26 (`css/css-highlight-api`) — массовый прогон

## Симптом

`crates/js/src/highlight_api.rs` (landed as "A-2 CSS Custom Highlight API
Phase 0", already merged to `main` before ROADMAP.md/STATUS-PN.md existed —
no open task tracks a Phase 1) installs a hand-rolled JS shim:

```js
global.Highlight = function Highlight(...ranges) {
  this.priority = 0;
  this.ranges = ranges;
};
if (!global.CSS) global.CSS = {};
global.CSS.highlights = {
  set: function(name, highlight) { ... },
  get: function(name) { ... },
  has: function(name) { ... },
  delete: function(name) { ... },
  clear: function() { ... },
};
```

This is neither of the two interfaces the spec (`css-highlight-api-1`)
actually requires:

- **`Highlight` is not Setlike.** It stores raw `.ranges`/`.priority` fields
  with no `.size`, `.add()`, `.delete()`, `.has()`, `.clear()`, `.entries()`,
  `.keys()`, `.values()`, `.forEach()`, or `Symbol.iterator` — every one of
  those throws `"... is not a function"` or resolves to `undefined`. The
  constructor also silently drops arguments into `.ranges` (a plain array)
  rather than deduplicating/validating them as a real `Set` would. It also
  has no `.type` attribute at all (spec: mutable, defaults to `"highlight"`,
  drives which `HighlightType` enum member the highlight paints as) —
  reading `.type` on a fresh `Highlight` gives `undefined`.
- **`CSS.highlights` is not a `HighlightRegistry` instance.** There is no
  global `HighlightRegistry` constructor at all (`window.HighlightRegistry`
  is `undefined`), so `CSS.highlights instanceof HighlightRegistry` and any
  spec test asserting the constructor's existence fail immediately. The
  object itself only has `set/get/has/delete/clear` — no `.size`, no
  Maplike iteration (`keys/values/entries/forEach/Symbol.iterator`), and no
  `highlightsFromPoint()`.

Confirmed by reading `crates/js/src/highlight_api.rs` directly (no grep
ambiguity — the shim is 40 lines, fully quoted above) and cross-checked
against the failing subtest messages, all of which name the exact missing
member (`CSS.highlights.keys is not a function`,
`CSS.highlights[Symbol.iterator] is not a function`,
`HighlightRegistry is in window got disallowed value undefined`,
`Highlight starts empty expected (number) 0 but got (undefined) undefined`
— i.e. `.size` missing).

## Масштаб

6 files / 30 subtests in `css/css-highlight-api` this slice:
`Highlight-setlike.html` (1 — `.size`), `Highlight-type-attribute.tentative.html`
(1 — `.type`), `HighlightRegistry-iteration.html` (15 —
`keys`/`values`/`Symbol.iterator`/`entries`/`forEach`),
`HighlightRegistry-iteration-with-modifications.html` (6 —
`Symbol.iterator`), `HighlightRegistry-maplike.html` (2 — no global
`HighlightRegistry` + `.size`), `HighlightRegistry-highlightsFromPoint.html`
(5 of its 7 fails — `.highlightsFromPoint` missing; the other 2 are
[BUG-533](BUG-533-FIXED.md)'s `StaticRange` and [BUG-480](BUG-480-OPEN.md)'s
`iframe.contentWindow`, not this bug).

Several other files in this slice show `"StaticRange is not defined"` or
setlike-adjacent symptoms but their *sole* cause this slice is
[BUG-533](BUG-533-FIXED.md) (`Highlight-multiple-type-attribute.html`,
`Highlight-setlike-tampered-Set-prototype.html`,
`HighlightRegistry-highlightsFromPoint-ranges.html`, `highlight-priority.html`)
— kept off this bug's file count to avoid double-attributing the same
subtest to two bugs. Not scoped beyond `css-highlight-api` — this is the
only WPT category built around the Highlight Registry surface.

## Что нужно

Rewrite the shim (or move it to native bindings, matching the pattern used
for other collection-like DOM types) so that:

1. `Highlight` implements the Setlike interface per spec — backed by a real
   `Set`-like store, exposing `size`/`add`/`delete`/`has`/`clear`/`entries`/
   `keys`/`values`/`forEach`/`Symbol.iterator`, all operating over the
   `(Range | StaticRange)` collection (needs [BUG-533](BUG-533-FIXED.md)'s
   `StaticRange` first for full coverage, though the setlike protocol itself
   doesn't strictly require it).
2. A global `HighlightRegistry` constructor exists, and `CSS.highlights` is
   an actual instance of it (`instanceof` must hold) implementing the
   Maplike interface (`size`, `keys/values/entries/forEach/Symbol.iterator`
   alongside the existing `get/set/has/delete/clear`).
3. `CSS.highlights.highlightsFromPoint(x, y, options?)` is implemented,
   returning `HighlightHitResult` entries for highlights present at the
   given viewport point (needs hit-testing against painted highlight ranges,
   not just registry bookkeeping).

The existing Rust-side `HighlightRegistry`/`Highlight` structs in
`highlight_api.rs` (used only by the crate's own unit tests today, never
consulted by the JS shim or by paint) could become the real backing store if
wired through native bindings instead of the current pure-JS closure-based
shim — worth checking during the fix whether paint (custom highlight
rendering) already depends on one representation over the other.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-highlight-api/` for the 6
affected files, `expected: FAIL` per actual subtest.

## Срез P3 2026-09-13 (закрытие большей части)

The row's status had been flipped to `FIXED 2026-09-11` at some earlier point
with no substantiating detail added and no matching commit; `grep -rn
"WeakMap\|HIGHLIGHT_STATE" crates/js/src/highlight_api.rs` on `HEAD`
confirmed the shim was still the original Phase-0 `.ranges` array/plain
`CSS.highlights` object described in the Симптом section above — the same
false-FIXED class as [BUG-533](BUG-533-FIXED.md), discovered alongside it.

Rewrote `HIGHLIGHT_API_SHIM` (`crates/js/src/highlight_api.rs`) from scratch:

1. **`Highlight` — real Setlike<(Range or StaticRange)>.** `.size`/`.has`/
   `.add`/`.delete`/`.clear`/`.keys`/`.values`/`.entries`/`.forEach`/
   `Symbol.iterator`, plus the mutable `.type` (`HighlightType` enum,
   out-of-enum values silently ignored per WebIDL enum-setter semantics) and
   `.priority` (coerced via `Number(v) | 0`) attributes the card's Симптом
   section flagged as absent.
2. **`HighlightRegistry` — real Maplike<DOMString, Highlight>.** A global
   `HighlightRegistry` constructor exists and throws `TypeError` when called
   with `new` (no spec constructor operation is defined); the single
   `CSS.highlights` instance is built by hand via `Object.create(
   HighlightRegistry.prototype)` + direct `WeakMap` state, bypassing the
   throwing constructor body. `CSS.highlights instanceof HighlightRegistry`
   holds. Same Maplike surface as (1).
3. **`highlightsFromPoint(x, y, options?)`** exists with full WebIDL
   argument validation (numeric coercion of `x`/`y`, `options.shadowRoots`
   iterable-of-`ShadowRoot` checks) — but always resolves to an empty array.
   Real hit-testing against painted highlight ranges needs paint to consume
   `CSS.highlights` at all, which it still doesn't (`highlight_name` on
   `DisplayCommand::DrawText`, `crates/engine/paint/src/display_list/
   text_highlight.rs`, is an unfed Phase-0 stub — confirmed by grepping for
   any writer of a non-`None` value, zero hits outside test fixtures). That
   remains the only open piece of this bug's originally-scoped 6 files.

Both `Highlight` and `HighlightRegistry` back their membership by a
singly-linked chain of plain entry objects (`{key, value, next, removed}`),
not a native `Set`/`Map` instance, for two independent reasons that both
turned out to be load-bearing during this slice:

- `Highlight-setlike-tampered-Set-prototype.html`/
  `HighlightRegistry-maplike-tampered-Map-prototype.html` freeze
  `Set.prototype`/`Map.prototype` with non-callable junk (`Set.prototype.add
  = true`, etc.) and still expect every method to work — nothing here may
  call through a native Set/Map instance's own prototype chain. Same
  reasoning `Headers` (`web_api_shim_mid_b.js`, BUG-369) already uses an
  array-backed list for.
- `Highlight-iteration-with-modifications.html`/
  `HighlightRegistry-iteration-with-modifications.html` require *live*
  iteration: an iterator must see insertions appended after it was created
  (as long as they land after its current position) and skip
  not-yet-visited deletions — a `.slice()` snapshot (the first draft of this
  fix) satisfies neither. `delete`/`clear` only flag an entry `removed` and
  never sever its `.next` pointer, so an iterator sitting on or behind a
  removed entry can still walk forward through the rest of the live chain —
  the same technique V8's own Map/Set iterators use internally. Iterators
  additionally defer reading the chain's `head` until the first `.next()`
  call ("lazy start"), so an iterator created while the collection is still
  empty observes entries added before that first call.

22 new end-to-end tests over the real V8 shim
(`crates/js/tests/cases/bug534_highlight_api.rs`) transcribe
`Highlight-setlike.html`, `Highlight-type-attribute.tentative.html`,
`HighlightRegistry-maplike.html`, `HighlightRegistry-iteration.html`
(keys/values/entries/`Symbol.iterator`/forEach subset),
`HighlightRegistry-iteration-with-modifications.html` and
`Highlight-iteration-with-modifications.html` (both live-iteration files —
the `Highlight` side isn't literally one of this bug's 6 originally-scoped
files, but needs the identical live-chain mechanism, so it's covered here
rather than left for [BUG-533](BUG-533-FIXED.md)'s card once StaticRange
stopped being the blocker for it), both tampered-prototype files, and the
argument-validation half of `highlightsFromPoint`. `cargo test -p lumen-js
--features v8-backend` 3632/3632 (one pre-existing unrelated flake under
parallel execution — `frame_bridge::tests::
inaccessible_bridge_mutation_does_not_mark_dirty`, passes in isolation, a
known global-dirty-registry test-ordering issue, not a regression). `cargo
clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
clean. `graphic_tests/dump_golden.py`/CPU snapshot drift not affected — pure
JS shim change; the pre-existing `snapshot_cpu` mismatch (7 unrelated pages)
was confirmed identical on a clean `main` via `git stash` A/B, not caused by
this change.

`.ini` dropped for `Highlight-setlike.html`,
`Highlight-type-attribute.tentative.html`, `HighlightRegistry-maplike.html`,
`HighlightRegistry-iteration.html`,
`HighlightRegistry-iteration-with-modifications.html`,
`HighlightRegistry-maplike-tampered-Map-prototype.html`,
`Highlight-setlike-tampered-Set-prototype.html` (all 6 of this bug's
originally-scoped files plus the tampered-Set file) — all subtests are
expected to pass now. `HighlightRegistry-highlightsFromPoint.html.ini`
narrowed to only the subtests that need real hit-testing (two
empty-result-only subtests dropped, since the stub already satisfies them
honestly, not by coincidence of a wrong assertion).
`HighlightRegistry-highlightsFromPoint-ranges.html.ini` kept as-is, just
re-attributed from BUG-533 to this bug's remaining scope. Live WPT run not
performed (no working `.venv`/`wss` patch in this slot) — confidence rests
on the transcribed unit tests above, not a real wptrunner pass; the next
step for whoever picks this back up is a live re-run to get exact
per-subtest `.ini` precision, then tackling the hit-testing gap (needs paint
to consume `CSS.highlights` — a real architectural addition, not a point
fix).

## Срез P3 2026-09-13 (реклассификация остатка)

Оставшийся объём (реальный hit-testing в `highlightsFromPoint()`) удовлетворяет
обоим условиям реклассификации `docs/probe-method.md` §8: функциональности нет
вовсе (paint не читает `CSS.highlights` ни в одном месте) и объём — новая связь
paint↔JS-registry, а не точечная правка. Заведена [GAP-HLHITTEST](../ROADMAP.md)
(`planned`), статус этой карточки — `OPEN (ДОРАБОТКА → GAP-HLHITTEST)`. Сама
Setlike/Maplike-реализация Highlight/HighlightRegistry (срез 2026-09-13 выше) не
затронута — полностью готова и работает.

## Закрытие 2026-09-26 (P1, GAP-HLHITTEST)

Остаток — реальный hit-testing в `highlightsFromPoint()` — сделан без участия
paint: точку сопоставляет с символом текстового узла layout
(`crates/engine/layout/src/text_geometry.rs`, таблица собирается same-tick
flush-ем лениво, натив `_lumen_text_at_point`), дальше шим проверяет, покрывает
ли диапазон этот символ (сравнение граничных точек DOM §5.2), отбрасывает
collapsed и невалидные StaticRange и сортирует по `priority`, затем по обратному
порядку регистрации. Попутно найден и исправлен дефект геометрии: wrap
склеивает слова соседних текстовых узлов одного стиля в один `InlineFrag`
(один `DrawText` — это намеренно, раздельные фрагменты сдвигали CPU-снапшот
`117-quotes`), а `source_node` у фрагмента один, поэтому второй inline-элемент
оставался без геометрии (нулевой `getBoundingClientRect`) — именно на нём
падал подтест «skips invalid StaticRanges». Теперь склейка записывает границу
(`InlineFrag::merged_sources`: байт, x, узел), а `collect_layout_rects`/
`collect_client_rects`/`collect_text_frag_rects` режут фрагмент по
`frag_source_spans`.

Живой WPT-прогон `css/css-highlight-api`: `HighlightRegistry-highlightsFromPoint-ranges.html`
1/1 (`.ini` удалён), `HighlightRegistry-highlightsFromPoint.html` 7/8 — остался
подтест с `display:none` iframe, он падает на `iframe.contentWindow === null`
(BUG-480) раньше, чем доходит до `highlightsFromPoint()`. Не моделируются:
перекрытие (occlusion) и фильтр `shadowRoots`. Отрисовка `::highlight()` — вне
этой карточки.
