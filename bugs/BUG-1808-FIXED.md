# BUG-1808 — `document.implementation.createHTMLDocument()` возвращает `Document`, у Chrome — `HTMLDocument`

**Статус:** FIXED 2026-11-17 (P3)
**Открыт:** 2026-11-14 (P3, попутный дефект при закрытии BUG-1779)
**Область:** js (DOMImplementation.createHTMLDocument, `web_api_shim_mid.js` — `_lumen_build_detached_document(Document.prototype, 'text/html')`)

## Симптом

`Object.prototype.toString.call(document.implementation.createHTMLDocument('t'))` →
`[object Document]`; у Chrome — `[object HTMLDocument]` (DOM LS: createHTMLDocument
возвращает HTMLDocument). Тот же документ не проходит `instanceof HTMLDocument`
и `doc.constructor === HTMLDocument`.

## Источник

Проба «до» фикса BUG-1779 (20 значений через `--dump-layout`, карточка BUG-1779 §Фикс):
`html-doc=[object Document]` — единственное расхождение с Chrome после расстановки
`@@toStringTag`. Это дефект не тегов, а выбора интерфейса фабрикой
(`createHTMLDocument`, `web_api_shim_mid.js:6550`: прототип захардкожен
`Document.prototype`; страница `document` получила `HTMLDocument` в BUG-1138,
детачед-фабрика — нет).

## Репродукция

```html
<!doctype html><pre id=out></pre>
<script>out.textContent = {}.toString.call(document.implementation.createHTMLDocument('t'));</script>
```

`--dump-layout` печатает `[object Document]`; ожидание — `[object HTMLDocument]`.

## Фикс (2026-11-17)

`createHTMLDocument` строит документ на `HTMLDocument.prototype`
(`web_api_shim_mid.js`) — единственный из трёх фабричных путей: `new Document()`
остаётся `Document`, `createDocument()` — `XMLDocument`. Интерфейс переживает
`cloneNode`: билдер `_lumen_build_detached_document` захватывает `proto` для
клона. Поведение не изменилось — `HTMLDocument.prototype` (BUG-1138) пустой
слой над `Document.prototype`, поэтому node-семантика, XPath-миксин на
`Document.prototype` и `defaultView: null` достигаются по той же цепочке.
Регресс-тест `create_html_document_is_an_html_document` в
`v8_bug1138_html_document.rs` (toStringTag, instanceof-цепочка, `constructor`,
клон).

## Замечание

Не форма BUG-1779 (там не хватало тегов при верном интерфейсе) — потому не чинилось
в том срезе: правка меняет тип инстанса, а не метаданные, и требует собственной
диф-обвязки.
