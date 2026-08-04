# BUG-603: table presentational hints (`bgcolor`/`background`/`bordercolor`/`cellspacing`) don't apply — `bgcolor` is static-only (never re-applied on `setAttribute`), the other three are entirely unimplemented

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style.rs::apply_bgcolor_presentational_hint` and the restyle-invalidation path that decides which attribute mutations trigger it)
**Найден:** P2, WPT-VENDOR-html-rendering, 2026-08-04

## Симптом

```
FAIL table bgcolor attribute is correct - assert_equals: expected "rgb(255, 0, 0)" but got ""
FAIL table background attribute is correct - assert_equals: expected "url(\"...\")" but got ""
FAIL table bordercolor attribute is correct - assert_equals: expected "rgb(255, 0, 0)" but got ""
FAIL table cellspacing attribute is correct - assert_equals: expected "10px" but got ""
```
(`non-replaced-elements/tables/table-attribute.html` — all 16
`{table,thead,tbody,tfoot,tr,td,th} × {background,bgcolor}` cases plus
`bordercolor`/`cellspacing` fail; by contrast the same file's `align`
(→`text-align`), `height`, `cellpadding` and `<col width>` presentational
hints on the same elements all **pass**, so this is not "table hints are
unimplemented" wholesale)

## Причина (два независимых дефекта, оба подтверждены живой пробой `--mcp-live-port`)

1. **`bgcolor` only applies from the initial HTML parse, never on a later
   `setAttribute`.** `apply_bgcolor_presentational_hint` (`style.rs:10935`)
   correctly reads the `bgcolor` attribute and sets
   `style.background_color` — but only during the style pass that runs
   off the parsed document. Probe:
   ```
   <table id="t1" bgcolor="red">  → getComputedStyle(t1).backgroundColor === "rgb(255, 0, 0)"   (correct)
   <table id="t2">                → t2.setAttribute('bgcolor','red');
                                     getComputedStyle(t2).backgroundColor === "rgba(0, 0, 0, 0)"  (wrong, stays transparent)
   ```
   Every WPT test in this file uses `setAttribute` at runtime (never a
   static HTML attribute), so it hits the broken path unconditionally.
   Whatever attribute-mutation → restyle invalidation list exists in the
   engine does not include `bgcolor` (contrast with `align`/`height`,
   confirmed dynamic and passing in the same file — so the general
   "attribute mutation triggers restyle" machinery works, `bgcolor`
   specifically is excluded from it, or `apply_bgcolor_presentational_hint`
   is only ever invoked from the one-time initial pass and not from
   whatever recomputes style on other attribute changes).

2. **`background` (image), `bordercolor`, `cellspacing` have no
   presentational-hint implementation at all**, static or dynamic —
   `grep -n '"bordercolor"\|"cellspacing"'
   crates/engine/layout/src/style.rs` returns nothing, and `"background"`
   only matches unrelated CSS shorthand-parsing code, not an
   attribute→style mapping. Confirmed missing even for a static
   `<table background="...">` in the initial HTML (not just the dynamic
   case above).

## Масштаб

`bgcolor`/`color`/`font-size` presentational hints (BUG-021,
`apply_text_color_presentational_hint`, `apply_font_element_presentational_hints`)
likely share whatever restyle-invalidation gap causes point 1 — anything
using `setAttribute` to flip a presentational-hint attribute after initial
load is suspect, not just `bgcolor`. Point 2 is three new, narrowly scoped
hints (HTML LS §15.3.8 tables) to add alongside the existing `bgcolor`
function.
