# BUG-324: `document.implementation` (DOMImplementation) отсутствует в WEB_API_SHIM

**Статус:** FIXED 2026-07-21
**Дата:** 2026-07-20
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** разбор `wpt_log.txt` — полный прогон `tests\wpt\run_report.py --binary "$BIN" --out .tmp\wpt-report-all.html --all` по сьюту `/dom/nodes/` (156 файлов).

## Симптом

`document.implementation` не определён — `grep -n "\.implementation\b" crates/js/src/dom.rs` не находит
ни присвоения свойства, ни объекта `DOMImplementation`. Отсюда во всех тестах:

```
FAIL DOMImplementation.createDocument(namespace, qualifiedName, doctype) -
Cannot read properties of undefined (reading 'createDocumentType')
FAIL DOMImplementation.createDocumentType(...) -
Cannot read properties of undefined (reading 'createHTMLDocument')
FAIL createElementNS test in XML document: null,null,null -
Cannot read properties of null (reading 'documentElement')
```

## Масштаб (по `wpt_log.txt`, сьют `/dom/nodes/`)

Итог прогона: `tests: 121/156 harness OK; subtests: 1081/4802 passed` (22.5% pass rate).
Топ-10 файлов по числу упавших сабтестов — **~2799 из 3721 (75%)** упавших сабтестов, и почти все
завязаны на `document.implementation`:

| Файл | Упавших сабтестов |
|---|---|
| `Element-classlist.html` | 655 (частично — фикстуры на XML-узлах через `createDocument`) |
| `Document-createElementNS.html` | 596 |
| `Document-characterSet-normalization-2.html` | 340 |
| `Document-characterSet-normalization-1.html` | 316 |
| `case.html` | 277 |
| `Document-createElement.html` | 147 |
| `DOMImplementation-hasFeature.html` | 136 |
| `Node-cloneNode.html` | 135 (XML/XHTML-варианты `cloneNode`) |
| `processing-instruction-attributes.html` | 126 |
| `Node-lookupNamespaceURI.html` | 70 |

`DOMImplementation-createDocument.html` и `DOMImplementation-createDocumentType.html` сами тоже FAIL
(см. симптом выше) — методы бросаются на первом же вызове.

## Ожидание

DOM Standard §4.5 (`Interface DOMImplementation`): `document.implementation` — живой объект с методами
`createDocumentType(qualifiedName, publicId, systemId)`, `createDocument(namespace, qualifiedName, doctype)`
(XMLDocument), `createHTMLDocument([title])`, `hasFeature()` (легаси, всегда `true`). Отсутствие этого
объекта каскадом валит все тесты, которые строят XML/XHTML-документ как одну из тестовых фикстур
(namespace-тесты `createElementNS`, `cloneNode` для XML, `lookupNamespaceURI`, processing instructions
и т.д.) — это не 10 независимых багов, а один пробел на входе.

## Замечание

`Element-classlist.html` (655) частично объясняется тем же — часть параметризаций теста строится на
"XML node with null namespace", т.е. тоже зависит от рабочего `createDocument`. Но там же виден и
отдельный паттерн: `classList.add/toggle/remove` с табами/переводами строк в токене на XML-узле не
бросает `InvalidCharacterError`/`SyntaxError` ("did not throw") — это может остаться отдельным дефектом
валидации токенов и не закрыться одним только добавлением `DOMImplementation`; перепроверить после
фикса этого бага, при необходимости завести отдельный BUG.

## Фикс (2026-07-21)

`crates/js/src/dom.rs` (`WEB_API_SHIM`, движко-независимый шим — общий для V8 и rquickjs):

- **`DOMImplementation`/`XMLDocument`** — новые неконструируемые интерфейс-глобалы (паттерн BUG-314),
  рядом с существующими `Document`/`DocumentType`.
- **`_lumen_build_detached_document(proto, contentType)`** — общий строитель detached-документа,
  вынесенный из BUG-321's `Document()` (который теперь просто вызывает его с
  `Document.prototype`/`'application/xml'`). Даёт `nodeType`/`nodeValue`/`DOCUMENT_NODE`/`childNodes`/
  `doctype`/`documentElement`/`implementation` (кэшируется в замыкании)/`URL`/`documentURI`/
  `compatMode`/`characterSet`/`charset`/`inputEncoding`/`contentType`/`location`, плюс
  `createElement`/`createElementNS`/`createTextNode`/`createComment`/`createDocumentFragment`/
  `appendChild`. Используется также `createDocument` (`XMLDocument.prototype`) и `createHTMLDocument`
  (`Document.prototype`).
- **`_lumen_make_detached_doctype(name, publicId, systemId, ownerDoc)`** — JS-only `DocumentType`
  (без арена-бэкинга) для результата `createDocumentType`; `ownerDocument` выставляется сразу на
  документ-владелец имплементации (не только при вставке) и переустанавливается через
  `__lumen_setOwner` при усыновлении другим документом.
- **`_lumen_make_dom_implementation(ownerDoc)`** — сам объект с 4 методами:
  - `createDocumentType` — валидация по наблюдаемому поведению реальных браузеров (WPT-таблица теста),
    которое ЗНАЧИТЕЛЬНО мягче строгой XML Name production (`_lumen_is_xml_name`, используемой
    `createProcessingInstruction`): пустая строка, ведущая цифра, произвольные символы и рваные
    двоеточия — все допустимы; бросает `InvalidCharacterError` только на пробельные символы и `>`.
  - `createDocument(namespace, qualifiedName, doctype)` — требует ≥2 аргументов (`TypeError` иначе,
    WebIDL required-arg), `contentType` зависит от namespace (`application/xhtml+xml`/`image/svg+xml`/
    `application/xml`), опционально усыновляет `doctype` и строит элемент через `createElementNS`.
  - `createHTMLDocument(title)` — строит html>head,body-скелет РЕАЛЬНЫМИ (арена-бэкнутыми, но не
    подключенными к корню живого дерева) узлами через `_lumen_create_element`/`_lumen_append_child`,
    так что `firstChild`/`lastChild` по нему работают как обычно; явный `undefined` в качестве `title`
    трактуется как отсутствующий аргумент (WebIDL trailing-optional-без-default правило) — `<title>`
    не создаётся.
  - `hasFeature()` — легаси no-op, всегда `true`.
- `document.implementation` (живая страница) — геттер с кэшем в `_lumen_document_implementation`, так
  что повторный доступ даёт тот же объект (`document.implementation === document.implementation`,
  WPT `Document-implementation.html`).

**Известное упрощение:** элементы, созданные через `createElement`/`createElementNS` detached-документа,
всё равно возвращают ГЛОБАЛЬНЫЙ `document` как `ownerDocument` (арена не хранит документ на узел) —
не спек-точно для `element.ownerDocument === detachedDoc`-проверок; аналогично `.prefix`/`.localName`
на элементах вообще не реализованы (существующий, более широкий пробел, не введённый этим фиксом).

7 юнит-тестов (`crates/js/src/dom.rs::dom::tests`): `document_implementation_is_cached_dom_implementation`,
`create_document_type_reflects_fields`, `create_document_type_rejects_invalid_name`,
`create_html_document_builds_skeleton`, `create_document_builds_xml_document`,
`create_document_requires_two_arguments`, `has_feature_always_true`.

## Результат прогона WPT после фикса

`tests\wpt\run_report.py --binary "$BIN" --out .tmp\wpt-report.html --all` по `/dom/nodes/`:

`tests: 121/156 → 126/156 harness OK`; `subtests: 1081/4802 → 1446/5446 passed` (денежник вырос —
больше файлов теперь регистрируют полный набор сабтестов вместо ERROR/TIMEOUT до setup).

Прямые бенефициары (полностью или почти полностью зазеленели):

| Файл | До | После |
|---|---|---|
| `DOMImplementation-hasFeature.html` | 1/137 | 137/137 |
| `DOMImplementation-createDocumentType.html` | 0/1 | 82/82 |
| `Document-implementation.html` | 0/2 | 2/2 |
| `DOMImplementation-createDocument.html` | 1/2 | 111/434 |

**Важно:** часть файлов из исходной таблицы «топ-10» диагноза (`Document-createElementNS.html`,
`case.html`, `Node-cloneNode.html`, `Document-characterSet-normalization-*.html`,
`Document-createElement.html`, `processing-instruction-attributes.html`,
`Node-lookupNamespaceURI.html`) остаются в основном красными и после фикса — при повторной проверке
их провалы оказались НЕ вызваны отсутствием `document.implementation`, а другой первопричиной:
преимущественно [BUG-322](BUG-322-FIXED.md) (`instanceof Element`/`HTMLElement` не работает для
нативных обёрток элементов — `Node-cloneNode.html`'s провалы буквально все `assert_true(original
instanceof HTMLXElement)`), которая уже заведена и остаётся OPEN. Первоначальная атрибуция в диагнозе
этого бага была основана на текстовом grep'е по логу, не на повторной проверке каждого файла — не
переносить её как факт без перепроверки.
