# BUG-581: `HTMLTableElement`/`HTMLTableRowElement`/`HTMLTableSectionElement` are bare interface stubs — zero table-specific DOM API

**Статус:** FIXED
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — the generated
"bare, non-constructible interface" loop covers `HTMLTableElement`,
`HTMLTableRowElement`, `HTMLTableCellElement`, `HTMLTableSectionElement`
alongside plain elements like `HTMLDivElement`; their prototypes chain
straight to `HTMLElement.prototype` with no table-specific members added
anywhere else in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04
**Исправлен:** P3, 2026-09-15

## Симптом

```
FAIL <test name> - table.createTBody is not a function
FAIL <test name> - table.deleteCaption is not a function
FAIL <test name> - table.createCaption is not a function
FAIL <test name> - table.createTHead is not a function
FAIL <test name> - table.insertRow is not a function
FAIL <test name> - table.getElementsByTagName is not a function
FAIL <test name> - tbody.insertRow is not a function
FAIL <test name> - tbody.deleteRow is not a function
FAIL <test name> - tr.insertCell is not a function
FAIL <test name> - tr.deleteCell is not a function
```

156 occurrences combined, all in `tabular-data/` (the category's harness
pass rate is otherwise high — 28/29 files parse and run — but only 9/153
subtests pass, i.e. the files load fine and then every table-manipulation
assertion fails).

## Причина

HTML LS §4.9 defines a sizeable convenience API on the table interfaces:
`HTMLTableElement.{caption, tHead, tFoot, rows, tBodies, createCaption,
deleteCaption, createTHead, deleteTHead, createTFoot, deleteTFoot,
createTBody, insertRow, deleteRow}`, `HTMLTableSectionElement.{rows,
insertRow, deleteRow}`, `HTMLTableRowElement.{rowIndex, sectionRowIndex,
cells, insertCell, deleteCell}`. None of it exists: the four table
interfaces are generated purely so `instanceof`/`'HTMLTableRowElement' in
window` resolve (`dom.rs:4608-4625`, the same generic loop used for
non-table elements like `HTMLDivElement` that genuinely have no
element-specific API). Even basic attribute-reflected properties
(`table.rows`, `tbody.rows`) don't exist — this isn't a missing-methods-only
gap, the live collections themselves are absent too. Contrast
`getElementsByTagName`, which is also called on `table` here but is the
already-tracked, more general [BUG-416](BUG-416-FIXED.md)
(`Element.prototype.getElementsByTagName` missing on every element, not
table-specific) — re-surfacing in this slice, not a new root cause.

## Масштаб

Whole feature surface, self-contained to `tabular-data/`. Given the harness
loads every file successfully, this is purely additive work (no engine-side
representation of tables is missing — layout already understands
`<table>`/`<tr>`/`<td>` structurally — just the four interface prototypes
need their spec methods/accessors wired, likely delegating to the same
child-walk/`_lumen_insert_before`-style primitives the rest of the shim
already uses for `insertBefore`/`appendChild`).

## Исправлено

Added the full HTML LS §4.9.11 convenience API in
`crates/js/src/shim/web_api_shim_tail_b.js`, plus a fifth table-only interface
(`HTMLTableCaptionElement`, missing from the same bare-stub loop in
`web_api_shim_mid.js` — needed for `caption`'s WebIDL nullable-interface type
check and for `instanceof HTMLTableCaptionElement`).

- **`rows`/`tBodies`/`cells`** are live `HTMLCollection`s built on the
  existing `_lumen_make_nid_collection` proxy machinery (the same one
  `select.options`/`form.elements` already use). `HTMLTableElement.rows`
  follows the spec's three-bucket order — a table's own thead rows, then a
  "table-or-tbody" middle bucket (a `tr` that is a *direct* child of `table`
  shares numbering with `tbody` children rather than getting its own), then
  tfoot rows — each computed fresh on every access, so mutating the table
  keeps the collection correct without a rebuild.
- **`caption`/`tHead`/`tFoot`** getters/setters mirror the real WebIDL
  binding for a nullable interface-typed attribute: the setter first runs an
  `instanceof` check (TypeError on mismatch — a `<div>` fails outright), then,
  for `tHead`/`tFoot` only, a second DOM-level check on the actual local name
  (`HierarchyRequestError` — a `<tbody>` passed as `tHead` clears the first
  gate, since `thead`/`tbody`/`tfoot` share one interface, and fails the
  second). A cycle check via the already-existing `_lumen_node_contains`
  helper catches the case where the assigned value already contains the
  table (also `HierarchyRequestError`) — this one is scoped to these three
  new setters only, not a generic fix to `insertBefore`/`appendChild`'s
  pre-existing lack of cycle detection (a separate, much larger gap).
- **`createCaption`/`createTHead`/`createTFoot`/`createTBody`** and their
  `delete*` counterparts follow the spec's per-interface insertion points:
  caption is literally the table's first child node (text nodes included,
  not just the first *element* child); thead goes immediately before the
  first element that is neither caption nor colgroup; tfoot appends at the
  end (or replaces the existing one in place); tbody always mints a new
  element (never reuses an existing one) and lands after the last tbody
  sibling.
- **`insertRow`/`deleteRow`** on both `HTMLTableElement` (rows-collection-
  relative, minting a fresh `<tbody>` only when the table has zero rows) and
  `HTMLTableSectionElement` (own-rows-relative, shared by thead/tbody/tfoot)
  bounds-check with `IndexSizeError`, matching `insertRow-method-0{1,2,3}.html`/
  `table-insertRow.html`/`remove-row.html`/the per-section WPT files.
- **`HTMLTableRowElement.insertCell`/`deleteCell`/`cells`/`cellIndex`** (the
  last one lives on `HTMLTableCellElement`) round out the row/cell side.
  `rowIndex` walks up to the owning *table* and indexes into its whole
  `rows`; `sectionRowIndex` uses the section's own rows for thead/tbody/tfoot
  children but the table's middle bucket for a `tr` that is a direct child of
  `table` — verified line-by-line against `the-tr-element/sectionRowIndex.html`'s
  16 nested/script-created cases.
- Every helper is HTML-namespace-aware (`_lumen_is_html_tag`), not just
  tag-name-aware: `_lumen_get_tag_name` upper-cases regardless of namespace
  (BUG-322), so a `<tr>`/`<td>`/`<tbody>` created via `createElementNS` in a
  genuinely foreign namespace must not be counted as a row/cell/section —
  `table-rows.html`'s "Complicated case" and `the-tr-element/cells.html`
  exercise exactly this.

**Residual, out of scope** — traced to separate, already-filed or clearly
distinct root causes, not this bug's "bare stub" defect:
[BUG-830](BUG-830-OPEN.md) (`createElementNS` with an arbitrary non-SVG/
MathML/empty namespace URI collapses to HTML) accounts for 5 subtests
(`createTBody.html`'s "namespaced tbody", `cells.html`, `table-rows.html`'s
"Complicated case", 3 of `caption-methods.html`'s namespace cases — the
foreign-namespace `<caption>`/`<tbody>`/`<tr>`/`<td>` those tests plant are
silently treated as real HTML elements). `table-insertRow.html`'s prefix
subtest is the documented `prefix` simplification from
[BUG-367](BUG-367-FIXED.md) (always `null`). `tBodies.html`'s one subtest
depends on `DOMParser`+`importNode` cross-document interaction not otherwise
exercised here. `caption-methods.html`'s cross-iframe-realm subtest needs a
brand check independent of the calling realm's constructor identity, which
plain `instanceof` cannot give — a generic multi-realm limitation, not
table-specific. `processing-model-1/{col-span-limits,rowspan-0,
rowspan-0-quirks}.html` (colSpan/rowSpan attribute reflection, clamping and
the layout algorithm's handling of `rowspan=0`) are an unrelated, unimplemented
feature; `col-span-limits.html`'s neighbor `span-limits.html` (65k-row stress
test) additionally crashes the WebDriver BiDi session outright, independent of
this fix.

`html/semantics/tabular-data/` via `tests/wpt/run_report.py --all --recursive`:
136/152 subtests passing (was 9/153; the harness itself now runs clean on
28/28 files instead of hanging on `tHead.html`'s cycle-attempt subtest, fixed
by the `_lumen_node_contains` guard above). New test file
`crates/js/tests/cases/bug581_table_api.rs` (17 tests) covers the same ground
at the unit level: row/cell collection ordering and namespace-blindness,
caption/thead/tfoot/tbody create/delete/get/set including the
TypeError-vs-HierarchyRequestError split, insertRow/deleteRow/insertCell/
deleteCell bounds and index semantics, rowIndex/sectionRowIndex/cellIndex.

`cargo test -p lumen-js --features v8-backend --test all` green (144/144, was
127/127 before this bug's tests), `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` clean.
