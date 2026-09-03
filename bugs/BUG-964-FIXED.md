# BUG-964: TRBL shorthand collapse on `style` write loses individual longhand reads when all four sides diverge

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js::_lumen_serialize_style`/`_lumen_shorthand_value`/`_lumen_parse_style`)
**Найден:** P1 2026-09-03, попутно при CSSOM-2 (первый срез, margin-*/padding-* `<length-percentage>` валидация)

## Механизм

`_lumen_serialize_style` (BUG-473, CSSOM §6.7.2 TRBL collapse) collapses
four present longhands (`margin-top`/`-right`/`-bottom`/`-left`, and the
same for `padding`/`border-width`/`-style`/`-color`) into their shorthand
form (`"margin: 10px auto -3px 0px;"`) whenever **all four** are present in
the in-memory `obj`, regardless of whether their values agree. This is
correct for `cssText`'s own round-trip (read the whole thing back, get the
whole thing), but the *stored* `style="…"` attribute text now contains only
the shorthand token. `_lumen_parse_style` (the plain `prop: value` splitter
used by `getParsed()`) has no shorthand-to-longhand expansion — it produces
a single `{margin: "10px auto -3px 0px"}` entry, not four. So a subsequent
`getPropertyValue('margin-bottom')` (or `e.style.marginBottom`) finds no
`margin-bottom` key AND `_lumen_shorthand_value(obj, 'margin-bottom')`
returns `undefined` (that table is keyed by shorthand name, not longhand
name) — the longhand read silently comes back `''`, as if never set.

## Repro

```js
var e = document.createElement('div');
e.style.marginTop = '10px';
e.style.marginLeft = '0';
e.style.marginRight = 'auto';
e.style.marginBottom = '-3px';   // completes all four TRBL sides
e.style.marginBottom;             // "" — expected "-3px"
```

Setting only `marginBottom` in isolation (nothing else on the same TRBL
group) round-trips correctly (`"-3px"` back) — the defect requires all
four sides of one collapsible group to be populated with **different**
literal values in the same declaration block.

## Масштаб

Not exercised by CSSOM-2's first slice's own repro (that used isolated
single-longhand assignments), but any real page/script that sets all four
margin (or padding/border-width/-style/-color) sides individually and then
reads one back via `getPropertyValue`/bracket access hits this. Likely
affects WPT's `margin-shorthand.html`/`padding-shorthand.html` longhand
readback assertions once CSSOM-2 covers those properties, and any script
pattern that sets sides one at a time (common when computed from four
separate values) rather than via the shorthand.

**Widened by CSSOM-2 slice 5 (2026-09-03, shorthand-VALUE parsing on
assignment):** the trigger is no longer "all four sides diverge" — a single
`style.margin = "10px"` assignment now sets all four longhands at once via
`_LUMEN_TRBL_SHORTHAND_CANON`/`_lumen_expand_trbl_shorthand`, even when all
four values are EQUAL, and `_lumen_serialize_style` collapses equal values
just as eagerly as divergent ones. So `e.style.margin = "10px";
e.style.marginTop` now also returns `""` instead of `"10px"` — every
`margin`/`padding`/`border-width`/`border-color` shorthand assignment hits
this, not just the individually-set-four-different-values pattern the
original repro described.

## Что нужно

Either (a) `_lumen_parse_style` must expand a shorthand token found in the
raw attribute text back into its longhand keys before returning `obj` (so
`getParsed()` always yields longhand-keyed state), or (b) `_lumen_serialize_style`
must not collapse into the shorthand text at all — keep longhand keys in
the stored attribute and only compute the shorthand form on-demand for
`cssText`'s/`getPropertyValue('margin')`'s own read. (b) is simpler and
avoids re-deriving parse ambiguity (a 4-value shorthand string does not
losslessly reconstruct which longhand `!important`/omitted state produced
it), but changes what `element.getAttribute('style')`/`outerHTML` show
after a longhand-only mutation session — needs a design decision, not a
quick patch.
