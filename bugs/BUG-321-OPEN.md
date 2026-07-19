# BUG-321: остаток DOM-конструкторов — Document/doctype/element-instanceof

**Статус:** OPEN
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
