# BUG-620: `Selection.prototype.toString()` always returns an empty string even when a valid Range is selected

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** dom (`crates/engine/dom/src/selection.rs::range_text`)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`) on a page with `<div id="a">hello
selection text</div>`:

```js
var s = getSelection();
s.selectAllChildren(document.getElementById('a'));
JSON.stringify({type: s.type, rangeCount: s.rangeCount, str: s.toString()});
// → {"type":"Range","rangeCount":1,"str":""}
```

`selectAllChildren` correctly builds a Range (`type` flips from `None` to
`Range`, `rangeCount` becomes 1 — the Range object itself is populated),
but `Selection.prototype.toString()` unconditionally returns `""` instead
of the selected text.

## Масштаб

This single defect drives the bulk of this category's failures:

- `inert-on-non-html.html`: 26 of 26 subtests FAIL, every one an
  `assert_equals` comparing `getSelection().toString()` against the
  expected selected text (`"non-inert"` or `""`) — the "non-inert" cases
  fail because the real content is stringified as `""`, and the
  already-`""`-expecting "inert" cases pass vacuously (which is why the
  category still shows 2 unattributed harness `PASS`-like results wrapped
  in the same file's own `2 duplicate test names` `ERROR` — a separate,
  pre-existing WPT-side quirk in that upstream test, not a Lumen bug).
- `inert-node-is-unselectable.html`: "Inert nodes cannot be selected." —
  `assert_equals: expected "I'm selectable." but got ""` — same root
  cause, masks the actual `inert`-and-selection behaviour the test means
  to check.
- `inert-with-modal-dialog-00{1,3}.html`'s four `checkSelection(...)`
  based subtests would hit the identical failure, but never get there —
  they error out first on the unrelated
  [BUG-384](BUG-384-FIXED.md) (named access on `window` for `dialog`/
  `wrapper`/`child`).

## Root cause (found, not yet fixed)

`Selection.prototype.toString()` (`dom.rs:4059`) calls the native binding
`_lumen_get_selection_text` (`v8_runtime.rs:3468`), which delegates to
`range_text()` (`crates/engine/dom/src/lib.rs:3255`). `selectAllChildren`
(`dom.rs:4043-4046`) sets `anchor = (node, 0)`, `focus = (node,
node_length(node))` — i.e. **both Range endpoints share the same
container, the element itself**, with offsets meant as *child-indices*
(DOM-spec Range semantics: when a boundary point's container is not a
CharacterData node, its offset indexes into `childNodes`).

`range_text`'s "same container" fast path (`lib.rs:3264-3271`) only
handles the case where that shared container is itself a **Text** node:

```rust
if start.container == end.container {
    if let NodeData::Text(s) = &doc.get(start.container).data {
        // char-offset substring logic
        ...
    }
    return String::new();   // <-- falls through here for an Element container
}
```

For `selectAllChildren(divElement)`, `start.container == end.container ==
divElement`, but `divElement`'s `NodeData` is not `Text` — the `if let`
pattern fails to match, and the function falls straight to
`return String::new()`. The "cross-container" branch further down (walking
`NodeId` index ranges and only accumulating `NodeData::Text` payloads) is
never reached here since the containers are equal, and would itself be
wrong for a purely-Element-vs-Element pair for the same reason: it never
resolves a *child-index* offset back into "which children fall inside the
range," only walks arena-index ranges of *already-known-to-be-text* nodes.

Fix shape: `range_text` needs an Element-container branch that resolves
`[start.offset, end.offset)` to a child-node slice via
`doc.get(container).children` and recurses/concatenates their text content
(mirroring `node_text_content`'s traversal), instead of assuming a shared
container is always `Text`.

Confirmed by reading the call graph: `Range.prototype.toString()`
(`dom.rs:3947-3949`) calls `_lumen_get_range_text`
(`v8_runtime.rs:3480-3486`), which builds the identical `DomRange` and
also bottoms out in `range_text` — so `range.toString()` has the exact
same gap independent of `Selection`, for any Range whose start/end
container is an Element rather than a Text node (not just ones produced
via `selectAllChildren`).

## Исправлено (2026-09-17, P3)

`range_text`'s same-container fast path (`crates/engine/dom/src/selection.rs`)
получила ровно предложенную выше Element-ветку: когда общий контейнер — не
`NodeData::Text`, `start.offset`/`end.offset` берутся как DOM-spec
child-индексы, клампятся к `doc.get(container).children.len()` и режут срез
`children[from..to]`; каждый ребёнок стрингифицируется (текстовый узел —
напрямую, элемент — через уже существующий `dom_collect_text`, тот же
обход, что у `node_text_content`), результат конкатенируется. Ветка чинит и
`Selection.toString()`, и `Range.toString()` одним изменением — оба бьются в
общий `range_text`, как и предполагалось в анализе выше.

Новые тесты в `crates/engine/dom/src/selection.rs::tests`:
`range_text_element_container_child_index_offsets` (плоский случай,
`selectAllChildren`-форма), `range_text_element_container_nested_elements`
(элемент внутри элемента), плюс регрессионный
`range_text_same_text_container_still_works` на исходном Text-пути.

`cargo test -p lumen-dom --lib selection::` 3/3,
`cargo clippy -p lumen-dom --all-targets -- -D warnings` чист.
