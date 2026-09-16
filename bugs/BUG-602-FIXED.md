# BUG-602: legacy `align` content attribute has no IDL reflection on any HTML element — `el.align` is `undefined` everywhere

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — reflection table introduced by BUG-383)
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

## Исправлено

`['align', 'align', 'string']` added to `_lumen_install_reflection` calls on
every interface the living standard actually gives an obsolete-but-conforming
`align` IDL attribute (checked against `html.spec.whatwg.org/multipage/obsolete.html`,
not just this bug's original grep-based list): `HTMLTableCaptionElement`,
`HTMLDivElement`, `HTMLHeadingElement`, `HTMLHRElement`, `HTMLIFrameElement`,
`HTMLImageElement`, `HTMLLegendElement`, `HTMLParagraphElement`,
`HTMLTableElement`, `HTMLTableSectionElement`, `HTMLTableCellElement`,
`HTMLTableRowElement`. `HTMLTableColElement` (`<col>`/`<colgroup>`) is
deliberately **not** in the list — the spec gives it `width` but not `align`,
unlike this bug's original write-up assumed by analogy; `HTMLObjectElement`
was also in the original write-up but is not in the spec's obsolete-`align`
table either, so it was left alone. Plain `string` reflection, no keyword
restriction — matches the spec's IDL, and is what `legend-align-justify-self.html`
needs (`legend.align.toLowerCase()` no longer throws). New tests:
`crates/js/src/dom/tests/v8_bug602_align_reflection.rs`.
