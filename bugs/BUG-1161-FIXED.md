# BUG-1161 — Отсоединённый документ не владеет своими узлами: `parentNode` корня `null`, `ownerDocument` — живой `document`, нет `createRange`

**Статус:** FIXED 2026-10-01 (P3)
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/shim/web_api_shim_mid.js`, `_lumen_build_detached_document`
(`:4161`): связь документ→ребёнок живёт в JS-массиве `_children`, арена не знает документа узла
(ограничение записано ещё в BUG-324)

## Симптом

После BUG-863 `dom/nodes/Node-properties.html` и `Node-contains.html` впервые проходят `setup`
(`run_smoke.py`, 2026-09-25): 642/726 и 1471/1482. Основная масса оставшихся FAIL — одна причина,
для документов из `createHTMLDocument()`/`createDocument()`/`new Document()`:

- `xmlElement.parentNode` / `foreignDoctype.parentNode` / `processingInstruction.parentNode` —
  `null` вместо документа; у PI/комментария/doctype в документе нет `previousSibling`/`nextSibling`;
- `xmlElement.ownerDocument`, `foreignPara1.ownerDocument` и т. д. — живой `document`
  (`Document node with 2 children`), а не свой;
- `foreignDoc.contains(foreignPara1)` и ещё 10 — тест строит ожидание по цепочке `parentNode`,
  которая обрывается на корне, поэтому получает `false`, а `contains` отвечает `true`;
- `foreignDoc.createRange is not a function` — `Range-selectNode.html` ERROR без сабтестов;
- у doctype и документа нет `parentElement`/`previousSibling`/`lastChild`/`textContent`
  (`undefined` вместо `null`), у doctype `firstChild` возвращает сам doctype.

## Что чинить

Документ узла в арене (или хотя бы корень отсоединённого документа как настоящий узел арены
`NodeData::Document`), чтобы `parentNode`/`ownerDocument`/обход шли по дереву, а не по JS-массиву.

## Исправление (P3, 2026-10-01)

Арена по-прежнему не знает документа узла; два факта вынесены в реестры шима
(`_lumen_doc_edge[nid]` — документ, держащий узел как прямого ребёнка; `_lumen_free_owner[nid]` —
документ-создатель свободного узла):

- `parentNode` корня отсоединённого дерева, `previousSibling`/`nextSibling` детей документа,
  `getRootNode()` — через реестр; doctype держит ссылку на документ через `__lumen_docParent`.
- `ownerDocument` — по вершине дерева: живой корень → `document`, иначе документ-ребёнок/создатель.
- JS-only Text/Comment/PI при вставке в документ получают узел арены (`_lumen_adopt_detached`).
- У документа и doctype `parentElement`/`previousSibling`/`nextSibling`/`firstChild`/`lastChild`/
  `textContent` — `null`.

Тест: `detached_document_children_link_back_to_document`; `cargo test -p lumen-js --features
v8-backend --lib` — 4656/4656. WPT-прогон `Node-properties`/`Node-contains` не делался.

Остаток: `createRange` на отсоединённом документе — [BUG-1233](BUG-1233-OPEN.md). Известное
упрощение: `ownerDocument` свободного поддерева определяется по его корню, усыновление поддерева
другим документом не обходит потомков.

