# BUG-581: `HTMLTableElement`/`HTMLTableRowElement`/`HTMLTableSectionElement` are bare interface stubs — zero table-specific DOM API

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:4608-4625` — the generated
"bare, non-constructible interface" loop covers `HTMLTableElement`,
`HTMLTableRowElement`, `HTMLTableCellElement`, `HTMLTableSectionElement`
alongside plain elements like `HTMLDivElement`; their prototypes chain
straight to `HTMLElement.prototype` with no table-specific members added
anywhere else in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

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
already-tracked, more general [BUG-416](bugs/BUG-416-OPEN.md)
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
