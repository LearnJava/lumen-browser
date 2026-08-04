# BUG-605: `<marquee>` has no `HTMLMarqueeElement` interface — `loop`/`scrollAmount`/`scrollDelay` IDL attributes missing, no UA `overflow:hidden` style

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — no `HTMLMarqueeElement` interface/prototype anywhere; `grep -n Marquee crates/js/src/dom.rs` zero hits) + layout (UA stylesheet has no `marquee { overflow: hidden !important }` rule)
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
