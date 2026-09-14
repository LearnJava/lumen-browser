# BUG-552: `document.compatMode` (and sibling document metadata properties) missing on the live `document` — only defined on synthetically-built detached documents

**Статус:** DUPLICATE → [BUG-358](BUG-358-FIXED.md)
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs:4780-4827` — `_lumen_build_detached_document`)
**Найден:** WPT-RUN-3 срез 34 (`ROADMAP.md`) — массовый прогон `css/css-position`

## Механизм

`compatMode`/`characterSet`/`charset`/`inputEncoding`/`contentType`/`URL`/
`documentURI` are only ever wired up inside `_lumen_build_detached_document`
(`dom.rs:4786-4827`), the constructor used for synthetic detached `Document`
instances (e.g. `document.implementation.createHTMLDocument()`). Grepping the
whole file for `'compatMode'` turns up exactly one hit — that one
`Object.defineProperty` call. The live/main `document` object (built through
a different code path) never gets this property defined at all, so
`document.compatMode` on the actual page document is `undefined` rather than
`"CSS1Compat"`/`"BackCompat"`. Even the detached-document stub is itself only
a hardcoded `"CSS1Compat"` literal — it never inspects whether the document
was parsed with a quirks-triggering doctype, so wiring it onto the live
document as-is would still not make quirks-mode pages report `"BackCompat"`
correctly; the doctype-sniffing logic (HTML parsing §"quirks mode") doesn't
exist anywhere in the shim yet.

## Симптом

`css/css-position/position-relative-aspect-ratio-002.html` — `"Document is in
quirks mode" - assert_equals: expected (string) "BackCompat" but got
(undefined) undefined`. Cascades into 2 more subtests in the same file
(`"Quirks: explicit width + aspect-ratio 2/1, top: 50%"` /
`"Quirks: auto width block + aspect-ratio 2/1, top: 50%"`) failing too,
since those rely on the page actually being in quirks mode (a doctype-less
document) for their containing-block-height special case to apply.

## Масштаб находки

1 file / 3 subtests in this slice. Likely affects any WPT test using
`document.compatMode` as a quirks-mode feature-detect (a common WPT idiom
across `quirks/` subdirectories in several categories, not searched here).

## Ревизия P3 2026-09-14 — закрыт как дубликат

Заявка (2026-08-04) описывает `dom.rs` до фикса [BUG-358](BUG-358-FIXED.md)
(2026-08-09), который сделал ровно это: `compatMode`/`characterSet`/
`charset`/`inputEncoding`/`contentType`/`URL`/`documentURI` — все на живом
`document` (`crates/js/src/shim/web_api_shim_mid.js:9800-9810`), не только на
отсоединённом. Само сомнение заявки — «даже сам детач-стаб — хардкод
`"CSS1Compat"`, doctype-сниффинга нигде нет» — тоже больше не верно:
`_lumen_get_document_compat_mode` (`crates/js/src/v8_runtime/install/dom_core.rs:92-98`)
читает `Document::mode()`, реальный флаг quirks-режима, который
`lumen-html-parser` вычисляет из DOCTYPE через `set_mode` (не хардкод).
Живая проверка через `cargo test -p lumen-js --features v8-backend bug552`
(7 новых тестов, `dom/tests/v8_bug552_document_metadata.rs`): все семь
свойств присутствуют на живом `document`, `compatMode` корректно даёт
`"BackCompat"` в `DocumentMode::Quirks`, `"CSS1Compat"` в `NoQuirks`/
`LimitedQuirks`, `characterSet`/`charset`/`inputEncoding` совпадают друг с
другом и с `Document::character_set`, `contentType` — с `Document::content_type`,
`URL`/`documentURI` — с URL страницы. Заявка не была сверена после фикса
BUG-358 — тот же класс дрейфа, что у BUG-482/512/523/533/534/544/555.
