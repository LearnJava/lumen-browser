# BUG-1732 — у документа из `document.implementation.createHTMLDocument()` нет `createNodeIterator`/`createTreeWalker`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P1, WPT-SHADOW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js`, ~стр. 14676 `createNodeIterator` определён только на главном `document`)

## Симптом

```
typeof document.createNodeIterator                                           → "function"
typeof document.implementation.createHTMLDocument('x').createNodeIterator    → "undefined"
typeof document.implementation.createHTMLDocument('x').createTreeWalker      → "undefined"
```

Остаток [BUG-415](BUG-415-FIXED.md) (отсоединённый документ без методов `Document`): тот восстановил `Node`-методы и `head`/`body`, но не `NodeIterator`/`TreeWalker`-фабрики.

## Как найдено

WPT-SHADOW-SCOUT (кластер K): `untriaged/shadow-trees/upper-boundary-encapsulation/ownerdocument-002.html` 0/18 («doc.createNodeIterator is not a function»), `dom-tree-accessors-001.html` 1/14; 3 id, 32 сабтеста. Вероятно тот же пробел у `DOMParser`-документов и у `Document.parseHTMLUnsafe`.

## Что делать

Определить `createNodeIterator`/`createTreeWalker` в общем прототипе `Document`, а не литералом главного `document`; проверить `createRange`, `createEvent`, `evaluate`, `importNode`, `adoptNode` на отсоединённом документе тем же списком (`Object.getOwnPropertyNames(Document.prototype)` против экземпляра).

## Как проверить

Три строки выше возвращают `"function"`; `run_corpus.py --prefixes shadow-dom/untriaged/shadow-trees/upper-boundary-encapsulation`.
