# BUG-682 — Storage Access API methods (`requestStorageAccess`/`hasStorageAccess`/`requestStorageAccessFor`/`hasUnpartitionedCookieAccess`) exist only on the live global `document`, not on documents built by `_lumen_build_detached_document`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:4423-4433` — defined only inside the live `var document = {...}` literal at `dom.rs:4182+`; `_lumen_build_detached_document`, `dom.rs:1795-1868`, the shared builder behind `DOMParser.parseFromString`/`DOMImplementation.createHTMLDocument`/`createDocument`/`createXMLDocument`, defines none of them)
**Найден:** P2, WPT-VENDOR-storage-access-api, 2026-08-06

## Симптом

Категория `storage-access-api` (`tests/wpt/storage-access-api/`, 40 отобранных
id) — вендорена и прогнана целиком (`run_report.py --all --root
storage-access-api --recursive`, 11:39, 40 id): **3/40 harness OK, 2/9
сабтестов**. Два не-`.https.` файла реально исполнились и оба падают на одном
и том же вызове:

```
FAIL [top-level-context] document.hasStorageAccess() should reject in a document
that isn't fully active. - promise_test: Unhandled rejection with value: object
"TypeError: createdDocument.hasStorageAccess is not a function"

FAIL [non-fully-active] document.requestStorageAccess() should reject when run
in a detached DOMParser document - CreateDocumentViaDOMParser(...)
.requestStorageAccess is not a function
```

Обе строки — один и тот же паттерн из `helpers.js`:

```js
function CreateDocumentViaDOMParser() {
  const parser = new DOMParser();
  const doc = parser.parseFromString('<html></html>', 'text/html');
  return doc;
}
```

`new DOMParser().parseFromString(...)` возвращает документ, собранный
`_lumen_build_detached_document` (`dom.rs:1795`), у которого нет
`requestStorageAccess`/`hasStorageAccess`/`requestStorageAccessFor`/
`hasUnpartitionedCookieAccess` вовсе — все четыре объявлены только внутри
рукописного литерала живого `document` (`dom.rs:4182+`, конкретно
`dom.rs:4423-4433`, doc-комментарий «Phase 0: always granted»).

## Причина

То же архитектурное расхождение, что и [BUG-358](BUG-358-OPEN.md)/
[BUG-359](BUG-359-FIXED.md): в шиме два независимых Document —
`_lumen_build_detached_document` (используется `DOMParser.parseFromString`,
`DOMImplementation.createHTMLDocument`/`createDocument`/`createXMLDocument`) и
рукописный литерал живого глобального `document`. BUG-358 нашёл расхождение в
одну сторону (`characterSet`/`compatMode`/… есть у detached, отсутствуют у
live); здесь — обратная сторона того же раскола: Storage Access API методы
добавлены только в live-литерал и никогда не зеркалились в
`_lumen_build_detached_document`. Оба направления подтверждают, что два
литерала не синхронизируются структурно — любое новое свойство/метод рискует
попасть только в один из двух.

Остальной сигнал прогона (2 из 5 сабтестов `requestStorageAccess-insecure`,
TestDriver-тест) — реконфирмация уже открытых дефектов, новых номеров не
заведено:
- `document.hasStorageAccess()`/`requestStorageAccess()` не проверяют
  небезопасный контекст (`assert_false: ... expected false got true`,
  `assert_unreached: ... call without user gesture`) — тот же корень, что
  [BUG-399](BUG-399-OPEN.md): `window.isSecureContext` захардкожен `true`
  безусловно (`dom.rs:8755`), поэтому Phase 0 шим «always granted»
  (`dom.rs:4423-4433`) не может отличить secure/insecure origin.
- `CreateDetachedFrame().requestStorageAccess()` падает на `null` —
  `frame.contentDocument` пуст, реконфирмация [BUG-480](BUG-480-OPEN.md)
  (`<iframe>` без отдельного browsing context).
- `elementDocument.contains is not a function` в TestDriver-тесте —
  реконфирмация [BUG-574](BUG-574-OPEN.md) (`Node.contains` отсутствует).

Остальные 37 id (все `.https.`) — TIMEOUT на уже задокументированном
TLS-гэпе `UnknownIssuer`, не дошли до навигации.

## Масштаб

Затрагивает любой код, вызывающий Storage Access API методы на документе,
полученном не из живого `window.document` — `DOMParser`-результаты,
`document.implementation.createHTMLDocument()`, XML-документы. Узкий по
площади (4 метода, один вызывающий паттерн в тестах), но диагностически
ценен как второй независимый пример класса BUG-358/359 — усиливает довод за
структурный фикс (слияние двух литералов/общий прототип), а не точечные
патчи по одному свойству за раз.

## Дальше

Fix scope: либо добавить те же четыре метода в
`_lumen_build_detached_document`, либо (предпочтительнее, per BUG-358 «Возможный
фикс») устранить сам архитектурный раскол — общий `Document.prototype`/фабрика,
которую оба пути используют. Не реализовано в этой сессии (P2-wpt вендорит и
обследует, фиксы — дорожка P3).
