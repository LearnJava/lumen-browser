# BUG-574: `Node.prototype.contains()` missing entirely (all node kinds, incl. `Document`)

**Статус:** FIXED (закрыто ревизией) 2026-09-14
**Компонент:** js (`crates/js/src/dom.rs:4503-4512` — `Node.prototype` shared-method
block; only `hasChildNodes` is wired there, `contains` was never added.
Live element/text/comment wrappers get their `[[Prototype]]` chained to
`Node.prototype` via `_lumen_build_element`, so a fix here reaches every node
kind)
**Найден:** P2, WPT-VENDOR-html-semantics-forms, 2026-08-04 (root-caused with a
standalone `--dump-layout` probe outside the WPT run)

## Симптом

Probe (`--dump-layout`, outside WPT):

```html
<div id="d"></div>
<script>
document.title = 'doc.contains=' + typeof document.contains +
  ' node.contains=' + typeof document.getElementById('d').contains +
  ' bodyContains=' + typeof document.body.contains;
</script>
```
→ `doc.contains=undefined node.contains=undefined bodyContains=undefined` —
`.contains` is not defined on *any* node kind, not even as a broken stub.
`tagName`/`nodeType` on the same objects work fine, so this isn't a general
wrapper problem, just this one method never being wired.

Inside WPT this surfaces indirectly through the vendored `/resources/testdriver.js`
helper `getInViewCenterPoint` (used by `test_driver.click()` and the `Actions`
API to compute a click point):

```js
// resources/testdriver.js:47-48
let elementDocument = element.ownerDocument;
if (!elementDocument.contains(element)) { ... }
```
→ `TypeError: elementDocument.contains is not a function`, thrown as an
unhandled promise rejection out of every `promise_test` that calls
`test_driver.click(...)` on an element.

## Причина

`Node.prototype` (`dom.rs:4503-4512`) only gets one shared method wired
(`hasChildNodes`, added for BUG-327). `contains(other)` — DOM §4.4, walk from
`other` up its ancestor chain via `parentNode` looking for `this` — was never
added. Same gap likely applies to its DOM §4.4 siblings
(`compareDocumentPosition`, `isSameNode`, `isEqualNode`, `getRootNode`) —
none of those strings appear in `dom.rs` either; not individually verified by
this report, worth checking in the same fix pass since they'd share the same
`this`-walks-`childNodes`/`parentNode` implementation pattern already used by
`hasChildNodes`.

## Масштаб

Directly counted in this category's run: **75 subtests across 36 files**
fail via the `test_driver.click()` → `elementDocument.contains` path alone
(`html/semantics/forms`, `run_report.py --all --root html/semantics/forms
--recursive`, 2026-08-04). Because the trigger is a shared testdriver.js
helper used by `test_driver.click()`/`Actions` everywhere — not something
specific to forms — this is very likely present as unexplained
`Unhandled rejection`/`TypeError` noise in every previously-run WPT category
that exercises click-based interaction (already-closed slices were not
re-audited for this specific message; flagging for awareness when triaging
old FAIL logs, not re-opening them).

**Расширено 2026-08-04 (P2, WPT-VENDOR-html-interaction):** the predicted
`getRootNode` sibling gap is confirmed — filed separately as
[BUG-599](BUG-599-FIXED.md) since it breaks a *different* call path
(`tools/wptrunner/wptrunner/testdriver-extra.js`'s `get_selector_array`,
used by `send_keys`/`get_computed_role`/`action_sequence`/etc., not just
`click()`'s `getInViewCenterPoint`). Fix both together — same `Node.prototype`
shared-method block, same implementation pattern.

## Закрытие (P3-ревизия, 2026-09-14)

Уже сделано — побочным эффектом [BUG-732](BUG-732-FIXED.md) (2026-08-10,
«шесть базовых DOM/CSSOM-API отсутствуют в шиме»), тот же фикс, что уже
закрыл [BUG-462](BUG-462-FIXED.md) с тем же симптомом (`elementDocument.contains
is not a function` из вендоренного `testdriver.js`) под другим номером. BUG-732
добавил `Node.prototype.contains`/`compareDocumentPosition`
(`crates/js/src/shim/web_api_shim_mid.js:3070-3072`) на цепочку прототипов, по
которой висят обёртки element/text/comment (`_lumen_build_element`), плюс
собственные копии на `document`-литерале (`web_api_shim_mid.js:9881-9882`) и на
`DocumentFragment` (`web_api_shim_mid.js:3909-3916`) — те два объекта, что не
наследуют `Node.prototype` вовсе. DOM §4.4 соседи, которые заявка просила
проверить в том же проходе, тоже на месте: `getRootNode` (закрыт отдельно как
[BUG-599](BUG-599-FIXED.md)), `isSameNode`/`isEqualNode`
(`web_api_shim_mid.js:7779-7787`, `10251-10254`).

Подтверждено юнит-тестами
`dom::tests::v8_bug732_node_and_collections::{contains_self_descendant_and_foreign_node, document_contains_element}`
(`cargo test -p lumen-js --features v8-backend v8_bug732_node_and_collections` —
7/7 OK на чистом `main`, код не менялся) — покрывают ровно сценарии этой заявки:
`document.contains(el)`, `el.contains(other)`, `Node.prototype` на живых
обёртках. Код не менялся, закрытие — устранение расхождения статуса.
