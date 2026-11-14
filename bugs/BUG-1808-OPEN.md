# BUG-1808 — `document.implementation.createHTMLDocument()` возвращает `Document`, у Chrome — `HTMLDocument`

**Статус:** OPEN
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

## Что сделать

Строить документ на `HTMLDocument.prototype` (как страницу `document` в BUG-1138),
не сломав `new Document()` (остаётся `Document` — сверить с Chrome) и
`createDocument()` (остаётся `XMLDocument`). Проверить WPT-семью
`DOMImplementation-createHTMLDocument.html` и наших тестов BUG-324/BUG-1138
(`v8_bug1138_html_document.rs` фиксирует поведение детачед-документов косвенно).

## Замечание

Не форма BUG-1779 (там не хватало тегов при верном интерфейсе) — потому не чинилось
в том срезе: правка меняет тип инстанса, а не метаданные, и требует собственной
диф-обвязки.
