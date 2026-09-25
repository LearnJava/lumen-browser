# BUG-1161 — Отсоединённый документ не владеет своими узлами: `parentNode` корня `null`, `ownerDocument` — живой `document`, нет `createRange`

**Статус:** OPEN
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
