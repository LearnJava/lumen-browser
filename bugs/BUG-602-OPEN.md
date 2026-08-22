# BUG-602: legacy `align` content attribute has no IDL reflection on any HTML element — `el.align` is `undefined` everywhere

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — IDL reflection table introduced by [BUG-383](BUG-383-FIXED.md))
**Найден:** P2, WPT-VENDOR-html-rendering, 2026-08-04

## Симптом

```
FAIL <fieldset><legend align=left>x</legend></fieldset> - TypeError: Cannot read properties of undefined (reading 'toLowerCase')
```
(`non-replaced-elements/the-fieldset-and-legend-elements/legend-align-justify-self.html`,
all 14 subtests — `legend.align.toLowerCase()` throws before any assertion
runs, so the test's own `justify-self` mapping logic is never exercised)

## Причина

`grep -n "'align'" crates/js/src/dom.rs` returns nothing — the deprecated
`align` IDL attribute (HTML LS §obsolete-but-conforming features;
reflected content attribute on `HTMLLegendElement`, `HTMLTableElement`,
`HTMLTableCellElement`, `HTMLTableRowElement`, `HTMLTableSectionElement`,
`HTMLImageElement`, `HTMLHRElement`, `HTMLIFrameElement`,
`HTMLObjectElement`, `HTMLParagraphElement`, `HTMLDivElement`, and more) is
absent from the reflection table BUG-383 introduced for `href`/`disabled`/
`maxLength`/etc. The content attribute itself is readable via
`getAttribute('align')`; only the `.align` JS property is missing.

This is a distinct mechanism from the table-cell `align`→`text-align`
**presentational hint** (that one already works — see
[BUG-603](BUG-603-FIXED.md), which is about a *different* set of table
attributes not reaching the CSS cascade at all).

## Масштаб

One reflection-table row (`{idl: "align", attr: "align", kind: string}`)
covers every affected interface at once, per the pattern BUG-383 already
established — no per-element special-casing needed unless a specific
interface constrains `align`'s value set (none of the WPT tests seen so far
require that).
