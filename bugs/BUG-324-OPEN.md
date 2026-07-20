# BUG-324: `document.implementation` (DOMImplementation) отсутствует в WEB_API_SHIM

**Статус:** OPEN
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

## Диагностика после фикса

Перезапустить `tests\wpt\run_report.py --binary "$BIN" --out .tmp\wpt-report-all.html --all` по
`/dom/nodes/` и сравнить `subtests: N/4802 passed` с текущим `1081/4802`.
