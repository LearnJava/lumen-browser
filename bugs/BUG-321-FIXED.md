# BUG-321: остаток DOM-конструкторов — Document/doctype/element-instanceof

**Статус:** FIXED 2026-07-20 (пункты 1–2; пункт 3 остаётся за [BUG-305](BUG-305-OPEN.md))
**Дата:** 2026-07-20
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Родитель:** [BUG-314](BUG-314-FIXED.md) (интерфейс-глобалы + Comment/Text/DocumentFragment)

## Симптом

[BUG-314](BUG-314-FIXED.md) выставил node-family интерфейсы как глобалы и сделал
конструируемыми `Comment`/`Text`/`DocumentFragment`. Остаются три куска, которые
намеренно отложены (крупнее focused-багфикса):

1. **Конструируемый `new Document()`** — сейчас `Document` — bare-интерфейс
   (`new Document()` бросает `TypeError: Illegal constructor`). WPT
   `Document-doctype.html` подтест «new Document()» строит detached-документ:
   `newdoc.appendChild(newdoc.createElement("html"))`, ожидает
   `newdoc.doctype === null`. Нужен отдельный detached-Document с собственным
   `createElement`/`appendChild`/`doctype`/`childNodes`.
2. **Живой `document.doctype`** — сейчас у `document` нет `doctype`. Подтест
   «Window document with doctype» ожидает
   `document.doctype instanceof DocumentType` и
   `document.doctype === document.childNodes[1]`. Нужен реальный
   `DocumentType`-узел, отражающий `<!doctype html>` разобранной страницы, и
   `document.childNodes` включающий его.
3. **`instanceof HTML*Element` для нативных элемент-обёрток** — обёртки
   (`_lumen_make_element`) остаются plain-объектами, поэтому
   `div instanceof HTMLDivElement`/`HTMLElement`/`Element`/`Node` = false.
   Общий долг с [BUG-305](BUG-305-OPEN.md): чтобы `instanceof` заработал, надо
   выставлять `[[Prototype]]` обёрток по тегу (`HTMLDivElement.prototype` и т.д.)
   — затрагивает всю систему элемент-обёрток, риск и объём вне P3-багфикса.

## Ожидание

Полный проход WPT `dom/nodes/Document-doctype.html` (оба подтеста) и
`el instanceof HTMLXElement` истинно для соответствующих тегов.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/Document-doctype.html
```

## Решение (пункты 1–2, оба подтеста `Document-doctype.html`)

Все правки — в `crates/js` (engine-agnostic shim + зеркальные нативы).

**Нативы (обоих движков — `dom.rs` rquickjs + `v8_runtime.rs`):**

- `_lumen_is_doctype(nid) -> bool` — узел является `NodeData::Doctype`.
- `_lumen_get_document_doctype() -> Option<u32>` — первый doctype-ребёнок
  корня документа (свойство `Document.doctype`).
- `_lumen_get_doctype_field(nid, which) -> Option<String>` — `name`/`publicId`
  (`"public"`)/`systemId` (`"system"`) узла DocumentType.

**JS-шим (`WEB_API_SHIM`):**

- `_lumen_make_doctype(nid)` — обёртка DocumentType (`nodeType 10`, прототип
  `DocumentType.prototype` → `instanceof` работает). Кэшируется в общем
  `_lumen_element_wrappers` по nid, поэтому `document.doctype` и
  `document.childNodes[1]` возвращают ОДИН объект (`===`), а `_lumen_gc_collect`
  очищает его как любую другую обёртку.
- `_lumen_make_node(nid)` — kind-aware обёртка ребёнка: doctype → `_lumen_make_doctype`,
  остальное → `_lumen_make_element`.
- `document.childNodes` — геттер поверх `_lumen_get_children(_lumen_root_nid)` через
  `_lumen_make_node`; `document.doctype` — поверх `_lumen_get_document_doctype`.
- `Document` сделан конструируемым: detached-документ с JS-массивом детей,
  `createElement`/`createTextNode`/`appendChild`, геттеры `doctype` (скан детей
  на `nodeType 10` → `null` у свежего) и `documentElement`.

**Тесты:** `document_doctype_is_document_type`, `document_doctype_null_when_absent`,
`new_document_constructor` (`crates/js/src/dom.rs`).

**Пункт 3** (`el instanceof HTMLXElement` для нативных элемент-обёрток) намеренно
не входит в этот фикс — это долг всей системы элемент-обёрток (обёртки остаются
plain-объектами), трекается как [BUG-305](BUG-305-OPEN.md). WPT
`Document-doctype.html` его не проверяет, оба его подтеста зелены только на 1–2.
