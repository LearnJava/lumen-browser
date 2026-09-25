# BUG-863 — `document.createCDATASection` отсутствует: одна недостающая фабрика роняет весь `dom/ranges` и половину `dom/traversal`

**Статус:** FIXED 2026-09-25 (P6)
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


## Реальный сайт (2026-09-24): youtube

Сайты ломает не фабрика, а **глобальный интерфейс**: полифилл ShadyDOM (`webcomponents-sd.js:102`)
делает `["Text","Comment","CDATASection","ProcessingInstruction"].forEach(a =>
Object.create(window[a].prototype))` → `Cannot read properties of undefined (reading 'prototype')`.
На youtube полифилл включается из-за членов не на прототипах интерфейсов (отдельный баг), и это его
первое падение. Репро `.tmp/compat/g1/yt_cdata.html`: Lumen `CDATASection=undefined`, Chrome
`function`, `createCDATASection` на HTML-документе — `NotSupportedError`, на XML — узел с
`nodeType 4`. Передан P6 по решению пользователя.

## Исправление (2026-09-25, P6)

**Статус:** FIXED 2026-09-25

- `lumen-dom`: CDATA-секция — обычный `NodeData::Text`, помеченный в `Document::cdata_sections`
  (множество индексов арены; `try_create_cdata_section`/`is_cdata_section`; метка переносится
  `deep_clone` и снимается `reclaim_dead_nodes`, так что переиспользованный слот её не наследует).
  Все текстовые пути (раскладка, `textContent`, диапазоны, `normalize`) не менялись.
- Натив `_lumen_create_cdata_section`/`_lumen_is_cdata_section`, `nodeName` `#cdata-section`
  (`crates/js/src/v8_runtime/install/dom_core.rs`).
- Шим: интерфейс `CDATASection` (наследник `Text`, не конструируется), `nodeType 4` и прототип в
  обёртке, `SHOW_CDATA_SECTION` в `NodeFilter`-обходе, фабрика `createCDATASection` у живого
  `document`, у каждого отсоединённого документа и у `VDocument` из `DOMParser`:
  `NotSupportedError` для `text/html`, `InvalidCharacterError` на `]]>`.
- `XMLSerializer` пишет `<![CDATA[…]]>` без экранирования (натив и `VNode`).
- Разбор `<![CDATA[…]]>` парсером не менялся — по-прежнему текст.

Тесты: `crates/js/src/dom/tests/v8_bug863_cdata_section.rs` (7).

### Замер (WPT, `dev-release`, та же машина, бинарь до/после)

| Прогон | До | После |
|---|---|---|
| `--all --root dom/ranges --recursive --processes 4` | 33/57 OK, 24 ERROR, 25/251 сабтестов | 46/57 OK, 12262/44063 сабтестов |
| `--all --root dom/traversal --recursive --processes 4` | 15/18 OK, 3 ERROR, 26/56 | 17/18 OK, 1031/1583 |
| `Document-createCDATASection-xhtml.xhtml` | ERROR `CDATASection is not defined` | OK 7/7 |
| `Document-createCDATASection.html` | 0/1 | 1/1 |
| `Node-compareDocumentPosition.html` | ERROR | OK 1444/1444 |
| `Node-contains.html` | ERROR | OK 1471/1482 |
| `Node-properties.html` | ERROR | OK 642/726 |
| `MutationObserver-textContent.html` | ERROR 3/4 | TIMEOUT 3/4 (→ BUG-1165) |

Все 24+3 ERROR с `createCDATASection is not a function` ушли. Категории впервые дошли до своих
утверждений; причины оставшихся отказов заведены отдельно:
[BUG-1159](BUG-1159-OPEN.md) (Range — заглушки сравнения и операций над содержимым),
[BUG-1160](BUG-1160-OPEN.md) (лимит арены на `Range-mutations-*`, три TIMEOUT),
[BUG-1161](BUG-1161-OPEN.md) (отсоединённый документ не владеет узлами),
[BUG-1162](BUG-1162-OPEN.md) (`createElement` в XML-документе),
[BUG-1163](BUG-1163-OPEN.md) (`wholeText`),
[BUG-1164](BUG-1164-OPEN.md) (`NodeIterator`/`TreeWalker`),
[BUG-1165](BUG-1165-OPEN.md) (`MutationObserver` и `DOMParser`).
Остаток ERROR в `dom/ranges` (`Range-cloneContents`/`extractContents`/`deleteContents`/
`surroundContents`/`insertNode` — `iframe.contentDocument`) — предел вложенных browsing context,
[BUG-480](BUG-480-OPEN.md).

Реальный сайт: `typeof CDATASection === 'function'`, `Object.create(window.CDATASection.prototype)`
проходит — первое падение ShadyDOM на youtube снято (юнит-тест
`interface_is_a_text_subclass_and_not_constructible`); сам полифилл включается из-за BUG-1122.
