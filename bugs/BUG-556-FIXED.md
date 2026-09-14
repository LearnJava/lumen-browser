# BUG-556: `Node.nextSibling`/`Node.previousSibling` are missing entirely on the live element/text/comment wrapper

**Статус:** FIXED 2026-09-14 (ревизия P3, дрейф трекера)
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_build_element`, around `:6542-6598` where `childNodes`/`firstChild`/`lastChild`/`nextElementSibling`/`previousElementSibling` are defined as getters on `_obj`)
**Найден:** P2, WPT-RUN-3 срез 39 (`css/cssom-view`), 2026-08-04

## Симптом

```
FAIL offsetTop/Left of empty inline elements should work as if they were not empty: 0
  TypeError: Cannot read properties of undefined (reading 'offsetLeft')
```

`offsetTopLeft-empty-inline.html`/`-empty-inline-offset.html`/
`-leading-space-inline.html`/`-trailing-space-inline.html` (30 subtests total
this slice) all do `var ref = target.nextSibling;` then read `ref.offsetLeft`
— `ref` itself is `undefined`, not the sibling `<span>` element (and not
`null`, which is what DOM §4.4 requires when there genuinely is no sibling).
Distinct failure mode from [BUG-476](BUG-476-OPEN.md) (wrong numeric
`offsetLeft` value) — here the property read throws before any geometry is
even involved.

## Причина

`_lumen_build_element`'s `_obj` (the generic live wrapper every
`getElementById`/`querySelector`/`firstChild`/etc. call returns) defines
`childNodes`, `firstChild`, `lastChild`, `nextElementSibling` and
`previousElementSibling` as getters (`dom.rs:6542-6598`) but never defines
`nextSibling`/`previousSibling` anywhere on that object — `grep -n
"nextSibling" crates/js/src/dom.rs` matches only `_TreeWalker.prototype
.nextSibling` (a different, unrelated tree-walking algorithm) and a
hardcoded `nextSibling: null` field on `MutationRecord` literals. Reading
`el.nextSibling` therefore returns plain-object `undefined` (no such own or
inherited property) rather than throwing or computing anything.

`nextElementSibling`/`previousElementSibling` (element-only, DOM
§4.2.7 `NonDocumentTypeChildNode`) are implemented correctly and can be
copied almost verbatim — the only difference is `Node.nextSibling`/
`previousSibling` (DOM §4.4) walk **all** child node types (text, comment,
element), not just elements, so the sibling list needs `_lumen_get_children`
(already used by `childNodes`/`firstChild`/`lastChild`), not
`_lumen_element_child_nids`.

## Масштаб находки

30 subtests this slice alone (`css/cssom-view`, all TypeErrors from the same
`ref.offsetLeft` pattern). `nextSibling`/`previousSibling` are basic,
extremely widely used DOM§4.4 traversal properties (any code walking a live
node list by hand, not via `children`) — likely affects other already-run
and future-run categories too; worth a grep for `.nextSibling`/
`.previousSibling` usage in WPT support scripts if this becomes a repeat
finding.

## Что нужно

Add `nextSibling`/`previousSibling` getters to `_obj` mirroring
`nextElementSibling`/`previousElementSibling` but over
`_lumen_get_children(pid)` instead of `_lumen_element_child_nids(pid)`.

## Ревизия P3 2026-09-14 — дрейф трекера, уже исправлено

Заявка описывает `dom.rs` до `P1/SPLIT-JS3` (шимы вынесены в
`crates/js/src/shim/*.js`, `include_str!`). `nextSibling`/`previousSibling`
уже определены как геттеры на `_LUMEN_WRAPPER_MEMBERS` в
`crates/js/src/shim/web_api_shim_mid.js:7725-7744`, над
`_lumen_get_children` (все типы узлов, а не только элементы) — реализация
попала в коммит `800cab893` («ES-модули из сети, nomodule, template.content
и узловые операции DOM», 2026-08-17), через 13 дней после подачи этой
заявки, но `BUGS.md` не обновили. Регрессионного теста не было —
добавлен `dom::tests::v8_childnode_traversal::node_next_previous_sibling_walk_all_node_types`
(обход текст → элемент → текст → элемент → текст, включая null-края
`firstChild.previousSibling`/`lastChild.nextSibling`). `cargo test -p
lumen-js --features v8-backend` — зелёный; `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` — чисто. Закрыт как
FIXED задним числом.
