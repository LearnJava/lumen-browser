Verification audit 2026-06-27 (all tests via `cargo test --lib -p <crate>` unless noted):

=lumen-core=: 278 passed
=lumen-image=: 152 passed
=lumen-paint=: 813 passed
=lumen-layout=: 3011 passed (incl. inline doctests)
=lumen-js=: 2324 passed
=lumen-css-parser=: all checks pass
=lumen-driver=: 111 passed; 2 pre-existing failures (BUG-250 font baseline, BUG-247 SVG AA) — unrelated to P3-color

## DONE (implemented + tested)

P3-color (ph3-color-management): Steps 1–7 complete, pushed 510b0e6d + bbd9802c. BUG-252/253/255/256/257 fixed.
P3-colormix: color_mix.rs (12 interpolation spaces) + parse_color_mix wired in style.rs; 30 tests pass.
P3-navapi: shell history stacks + JS Navigation singleton + BUG-255/256 fixes; navigate_to/back/forward/navigate_by/replaceState/pushState fully wired.
P3-bfcache: HTML-snapshot store/retrieve + scroll restore + pageshow persisted flag.
P3-varfonts: BUG-109 fixed; vector outlines render path in femtovg + wgpu; variation axes parsed.
P3-regprop: @property parsing + syntax validation + inherits/initial-value inheritance; 30+ tests pass.
P3-subgrid: Grid L2 subgrid columns/rows inherited track sizes + collect_subgrid_items; 9 tests pass.
P3-has: :has() selector parsing + cascade matching; 86 tests pass.
P3-nesting: CSS Nesting L1 parser (explicit `&` + implicit descendant/child/relative combinators + nested at-rules); 17 parser tests + cascade tests pass.
P3-textwrap: text-wrap-mode / text-wrap-style / text-wrap shorthand parsed + inherited; balance widening/narrowing tests pass (5 tests).
P3-multicol: column-count / column-width / column-gap / column-fill balance/auto / column-span all; 9 tests pass.
P3-resizeobs: ResizeObserver JS singleton + `_lumen_deliver_resize_observers` + border-box-size entries; tests pass.
P3-intersectobs2: IntersectionObserver v2 (threshold, rootMargin, unobserve, lazy-image integration); 10 tests pass.
P3-streams: WritableStream + sink/pipeThrough + backpressure; 59 stream-related tests pass.
P3-webcrypto: SubtleCrypto HMAC + ECDSA + AES-GCM + import/export JWK/PKCS8; 16 tests pass.
P3-weblocks: LockManager + query/request/ifAvailable; 6 tests pass.
P3-broadcast: BroadcastChannel name-isolation + message delivery + close; 14 tests pass.
P3-clipboard: Async Clipboard read/write text; tests pass.
P3-cookiestore: Cookie Store API partitioned by origin; tests pass.
P3-cacheapi: CacheStorage + Cache + match/put/delete/keys on sqlite backend; 32 tests pass.
P3-permissions: Permissions.query + onchange + per-name grants; 10 tests pass.
P3-notifications: Notification.requestPermission + show + SW getNotifications; 26 tests pass.
P3-offscreencanvas: OffscreenCanvas transfer + 2D native from ImageData; 24 tests pass.

## IN PROGRESS / NEEDS WORK

P3-bfcache: HTML snapshot works; JS heap freeze (Frozen payload) is stubbed — requires heap serialization for full bfcache.
P3-anchorpos: anchor-name / position-anchor parsed; `anchor()` function resolution exists but L1 `inset-area` / `position-area` may be partial.
P3-scope: CSS @scope at-rule may not be parsed/applied (no tests found).
P3-stylequery: Container style queries (`style()` function) likely not implemented (no tests found).
P3-counterstyle: @counter-style at-rule may be parsed but not wired to marker generation.
P3-fragmentation: break-inside / widows / orphans props likely stubbed.
P3-initialletter: initial-letter drop-cap not implemented in layout.
P3-vertical: CSS writing-mode vertical text — Phase 2 (vertical inline text flow + sideways glyphs) implemented. `wrap_inline_run_vertical` + `lay_out_vertical_inline_run` added; `text_orientation` field propagated through display list; existing tests pass. Paint-side glyph rotation (90° CW/upright/CJK mixed) deferred to Phase 2b.
P3-pushapi: Push API file exists but may be stub-level.
P3-reporting: Reporting API stub-level.
P3-earlyhints: 103 Early Hints + fetch Priority Hints — may need server-side simulation.

Next concrete task: implement one of the clearly-incomplete CSS layout features
(e.g. P3-vertical, P3-initialletter, P3-fragmentation, P3-multicol rule/gap completeness)
or a JS API stub upgrade (e.g. P3-pushapi, P3-reporting, P3-structuredClone).
