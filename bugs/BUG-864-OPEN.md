# BUG-864 — namespace-навигация узла отсутствует целиком: `lookupNamespaceURI`, `lookupPrefix`, `isDefaultNamespace`

**Статус:** OPEN
**Заведён:** 2026-08-23 (P2, `WPT-VENDOR-dom-rest` — первый прогон довендоренной категории `dom`)
**Область:** `crates/js/src/dom.rs` (`WEB_API_SHIM`, прототипы `Node`/`Element`/`Document`) — `grep -rn --include="*.rs" "lookupNamespaceURI\|lookupPrefix\|isDefaultNamespace" crates/` даёт **ноль** совпадений во всём воркспейсе
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
document.documentElement.lookupNamespaceURI(null);   // TypeError: node.lookupNamespaceURI is not a function
document.documentElement.isDefaultNamespace("http://www.w3.org/1999/xhtml");  // TypeError
document.documentElement.lookupPrefix("http://www.w3.org/1999/xhtml");        // TypeError
```

DOM §4.4 определяет все три метода на `Node` (алгоритмы «locate a namespace»,
«locate a namespace prefix»), с отдельными ветками для `Element`, `Document`,
`DocumentType`, `Attr` и прочих узлов.

## Прямое измерение

Прогон `run_report.py --all --root dom --recursive` (2026-08-23, Linux, dev-release,
`main` = `99c771d14`): `/dom/nodes/Node-lookupNamespaceURI.html` — статус гарнеса `OK`,
но **70 сабтестов FAIL** одним из двух сообщений: `node.lookupNamespaceURI is not a
function` (34) и `node.isDefaultNamespace is not a function` (36). Ни одного PASS в
файле. `lookupPrefix` в сообщениях не появляется только потому, что тест до него не
доходит — метод отсутствует ровно так же.

## Масштаб

Один файл на 70 сабтестов в `dom/nodes`, но методы общедоступные (`Node.prototype`),
поэтому цена не ограничена этим тестом: то же отсутствие ловят XML/XHTML-сценарии
и любой код, разрешающий префикс по узлу. Отдельного гэпа в `crates/engine/dom`
нет — namespace у элементов хранится (`createElementNS` работает, `namespaceURI`
читается), недостаёт именно трёх алгоритмов обхода предков.

## Что чинить

`Node.prototype.lookupNamespaceURI` / `lookupPrefix` / `isDefaultNamespace` в
`WEB_API_SHIM`, поверх уже имеющихся `namespaceURI`/`prefix`/`parentNode`:
подъём по предкам с проверкой `xmlns`/`xmlns:*`-атрибутов, ветка `Document` →
`documentElement`, `Attr` → `ownerElement`, `DocumentType`/`DocumentFragment` → `null`.
Алгоритмы целиком описаны в DOM §4.4 и не требуют изменений в arena-DOM.
