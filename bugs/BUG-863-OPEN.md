# BUG-863 — `document.createCDATASection` отсутствует: одна недостающая фабрика роняет весь `dom/ranges` и половину `dom/traversal`

**Статус:** OPEN
**Заведён:** 2026-08-23 (P2, `WPT-VENDOR-dom-rest` — первый прогон довендоренной категории `dom`)
**Область:** `crates/js/src/dom.rs` — `grep -rn --include="*.rs" "createCDATASection\|CDATASection" crates/` даёт **ноль** совпадений во всём воркспейсе; у `document` есть `createTextNode` (`dom.rs:2474`), `createComment` (`:2479`), `createDocumentFragment` (`:2484`), `createProcessingInstruction` (`:2743`) — соседней `createCDATASection` нет ни в шиме, ни в arena-DOM (узла типа CDATA не существует)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
const xml = document.implementation.createDocument(null, "root", null);
xml.createCDATASection("1234");   // TypeError: xmlDocument.createCDATASection is not a function
```

DOM §4.5 требует `createCDATASection(data)` у любого `Document` (для HTML-документа —
`NotSupportedError`, для XML — узел `CDATASection extends Text`).

## Почему это важнее, чем «ещё один отсутствующий метод»

Вызов стоит в `tests/wpt/dom/common.js:60-61` — в `setupRangeTests()`, общем
`setup()`-хелпере **всех** тестов `Range`/`NodeIterator`/`TreeWalker`. Исключение
летит из `setup`, то есть до регистрации первого `test()`, и `testharness.js`
рапортует статус гарнеса `ERROR` вместо списка сабтестов: файл не даёт ни одного
результата, даже по тем проверкам, которые к CDATA отношения не имеют.

## Масштаб (прогон `run_report.py --all --root dom --recursive`, 2026-08-23, Linux, dev-release)

| Каталог | id со статусом ERROR из-за этого вызова |
|---|---:|
| `dom/ranges/` | 24 (**все** ERROR категории; сабтестов проходит 10 из 251) |
| `dom/traversal/` | 3 (`NodeIterator.html`, `NodeIterator-removal.html`, `TreeWalker.html`) |
| `dom/nodes/` | 4 (`Node-contains`, `Node-compareDocumentPosition`, `Node-properties`, `MutationObserver-textContent`) |
| **итого** | **31 id** |

Это крупнейшая единичная причина в довендоренной части `dom` — 31 из 86 не-OK id.
`dom/nodes/Document-createCDATASection.html` при этом отрабатывает (`OK`): он ловит
исключение своими `assert_throws_*`, поэтому в дневном отчёте выглядел безобидно.

## Что чинить

1. Тип узла `CDATASection` (наследник `Text`) в arena-DOM `lumen-dom` — либо, как
   минимальный шаг, представлять его текстовым узлом с флагом.
2. `document.createCDATASection(data)` в `WEB_API_SHIM` рядом с `createComment`:
   бросать `NotSupportedError` для HTML-документа, создавать узел для XML,
   `InvalidCharacterError` на `]]>` внутри `data` (DOM §4.5).
3. Сериализация `<![CDATA[…]]>` в `XMLSerializer` — иначе `outerHTML`-проверки
   тех же тестов останутся красными.

Пока не исправлено, `dom/ranges` в отчёте нечитаем: 24 ERROR маскируют реальное
состояние `Range` (по 3 сабтестам, которые всё же успели пройти, видно, что
базовый `Range` живой).
