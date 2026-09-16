# BUG-605: `<marquee>` has no `HTMLMarqueeElement` interface — `loop`/`scrollAmount`/`scrollDelay` IDL attributes missing, no UA `overflow:hidden` style

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — reflection table introduced by BUG-383) + layout (`crates/engine/layout/src/style/cascade.rs` — post-cascade forced override, same shape as `apply_forced_colors_mode`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL marquee_loop_normal - assert_equals: The value of loop should be 2. expected (number) 2 but got (undefined) undefined
FAIL The scrollamount is a normal value - assert_equals: The value of scrollamount should be 10. expected (number) 10 but got (undefined) undefined
FAIL The scrolldelay attribute is a string - assert_equals: The delay time should be 85ms. expected (number) 85 but got (undefined) undefined
FAIL Event handler IDL attributes must not be implemented - HTMLMarqueeElement is not defined
FAIL Marquee should have overflow: hidden !important in the UA stylesheet - string "" is not a function
```
(`obsolete/requirements-for-implementations/the-marquee-element-0/marquee-{loop,scrollamount,scrolldelay,overflow,events-historical}.html`)

## Причина

HTML LS §obsolete requires `<marquee>` to expose a dedicated `HTMLMarqueeElement`
interface with reflected `loop` (`long`, default `-1`), `scrollAmount`
(`unsigned long`, default `6`), `scrolldelay`→`scrollDelay` (`unsigned long`,
default `85`) IDL attributes (each with clamping/parsing rules per the
"marquee" reflection algorithm — non-numeric or out-of-range content
attribute values fall back to defaults), plus a UA stylesheet rule forcing
`overflow: hidden !important` regardless of any author `overflow` value.
Lumen has none of this: `document.createElement('marquee')` produces a
plain `HTMLElement` with no dedicated prototype (`HTMLMarqueeElement` isn't
even a global constructor), so every IDL attribute on it is `undefined`,
and there's no UA-stylesheet entry forcing `overflow: hidden`.

## Масштаб

Self-contained, 5 files in `the-marquee-element-0/`, ~13 subtests. No other
category depends on `<marquee>` in this corpus.

## Исправлено

`HTMLMarqueeElement` added to the existing stub-ctor list (same shape as
`HTMLAreaElement`/`HTMLDetailsElement`/… at `web_api_shim_tail_b.js:1202`) and
wired into `_lumen_html_tag_prototypes['MARQUEE']`. `scrollAmount`/
`scrollDelay` are plain `unsigned long` reflections (`['scrollAmount',
'scrollamount', 'ulong', 6]` / `['scrollDelay', 'scrolldelay', 'ulong', 85]`)
— negative or unparseable falls back to the default via the existing generic
`'ulong'` kind. `loop` needed a bespoke getter/setter instead: the spec's
parsed value must fall back to `-1` not just when it overflows `i32` but for
*any* value less than 1 (`loop="-2"` → `-1`, confirmed by `marquee-loop.html`
`marquee_loop_less_than_1`), which the generic `'long'` kind does not do (it
only clamps on actual range overflow, so `-2` would pass through verbatim).
No `onstart`/`onfinish`/`onbounce` event handler IDL attributes were added —
HTML LS requires the interface to omit them, and this engine never fires
those events, so the omission needed no extra code.

UA `overflow: hidden !important`: this codebase has no textual UA
stylesheet, only hand-written Rust in `cascade.rs`. The precedent for
`!important`-strength unconditional UA behavior is Forced Colors Mode
(`apply_forced_colors_mode`, runs post-cascade so it beats even inline
`style=""`) — the marquee override follows the same shape, forcing
`overflow_x`/`overflow_y` to `Overflow::Hidden` for the `marquee` tag right
after `coerce_overflow_axes`.

New tests: `crates/js/src/dom/tests/v8_bug605_marquee_reflection.rs` (V8,
interface/loop/scrollAmount/scrollDelay) and three cases appended to
`crates/engine/layout/src/style/tests/ua.rs` (forced overflow, including
against an author inline-style override). `<marquee>` does not appear
anywhere in `graphic_tests/`, so the layout change cannot affect any existing
pixel/display-list golden — confirmed via `python graphic_tests/dump_golden.py`
(4/12 mismatches, matching the pre-existing unrelated baseline drift, no new
ones).
