# BUG-1162 — `createElement` в XML-документе создаёт HTML-элемент: `tagName` в верхнем регистре, namespace XHTML

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-863](BUG-863-FIXED.md))
**Область:** js — `crates/js/src/shim/web_api_shim_mid.js:4305`, `doc.createElement` у
отсоединённого документа: `_lumen_create_element(String(tag).toLowerCase())` для любого
`contentType`

## Симптом

DOM §4.5 `createElement`: имя приводится к нижнему регистру и namespace — XHTML **только** в
HTML-документе; в XML-документе (`createDocument(null, …)`, `new Document()`) элемент получает
namespace `null`, имя как есть. `dom/nodes/Node-properties.html` (2026-09-25, после BUG-863):

```
FAIL xmlElement.tagName      - expected "igiveuponcreativenames" but got "IGIVEUPONCREATIVENAMES"
FAIL xmlElement.nodeName     - то же
FAIL xmlElement.namespaceURI - expected null but got "http://www.w3.org/1999/xhtml"
```

`xmlDoc.createElement("fooBar").localName` — `"foobar"` вместо `"fooBar"`.

## Что чинить

В `_lumen_build_detached_document` ветвиться по `contentType`: для не-`text/html` —
`_lumen_create_element_ns('', tag)` без приведения регистра (с проверкой имени по XML Name).
