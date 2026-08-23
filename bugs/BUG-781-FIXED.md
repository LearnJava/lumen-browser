# BUG-781: `DOMParser.parseFromString` игнорирует XML MIME-типы — всегда гоняет HTML-токенизатор и заворачивает корень в синтетический `<html>`

**Статус:** FIXED 2026-08-23 (P1)

**Компонент:** js (`crates/js/src/dom_parser.rs:526-567`, `_vBuildDocument`; `DOMParser.prototype.parseFromString`, `dom_parser.rs:850-861`)

**Найден:** P2, WPT-VENDOR-xml 2026-08-18 (`run_report.py --all --root xml --recursive`, 56.79 с, 3/7 harness OK, 4/190 сабтестов)

## Симптом

`new DOMParser().parseFromString(xml, 'text/xml' | 'application/xml' | 'image/svg+xml' | 'application/xhtml+xml')`
принимает `mimeType`-аргумент (валидирует его против списка из 5 значений,
`dom_parser.rs:853-859`), но затем безусловно зовёт `_vBuildDocument`, которая
для любого MIME гоняет один и тот же HTML-токенизатор (`_vParseHTML`). Тот
ищет литеральный элемент `<html>` на верхнем уровне; если его нет (обычный
случай для XML — корень называется как угодно, `<a>`/`<b>`/`<rss>`/…), весь
распарсенный контент заворачивается в синтетические `<html><head></head>
<body>...</body></html>` (`dom_parser.rs:546-558`, ветка `else`). В результате
`documentElement.tagName`/`.nodeName` для любого XML-документа — всегда
`"HTML"`, независимо от реального корневого тега во входной строке; настоящее
дерево документа проваливается на два уровня внутрь (`html > body > <реальный
корень>`), `documentElement.firstChild` — это `<head>`, а не первый узел
реального документа.

Живое воспроизведение (`tests/wpt/xml/xml-prolog-accepted-versions.html`):

```js
var d = new DOMParser().parseFromString('<?xml version="1.0"?>\n<a></a>', 'text/xml');
d.documentElement.tagName   // "HTML", ожидание — "a"
```

и (`tests/wpt/xml/eol-normalization.html`):

```js
var d = new DOMParser().parseFromString('<a>\r\n\t<b>x</b></a>', 'text/xml');
d.documentElement.nodeName             // "HTML", ожидание — "a"
d.documentElement.firstChild.nodeValue // null (это <head>), ожидание — "\n\t"
```

Побочный эффект — 4 из 10 сабтестов `xml-prolog-accepted-versions.html`
проходят **случайно**: `assert_not_equals(...tagName, "x")` для отклоняемых
версий XML (`10.0`/`100`/`2.0`/`17.0`) всегда истинно, раз `tagName` в любом
случае `"HTML"` — валидации номера версии в прологе нет вообще, тест не
проверяет то, что называет (см. паттерн
`feedback_probe_must_not_name_what_it_defines` / зелёный тест маскирует
дефект).

## Root cause

`_vBuildDocument(html, mimeType)` (`dom_parser.rs:526-567`) записывает
`doc.contentType = mimeType` (косметика — используется только `.contentType`
геттером, нигде не влияет на парсинг), но и `root = _vParseHTML(html, doc)`,
и последующий поиск/синтез `html`/`head`/`body` не зависят от `mimeType`.
Файл сам документирует это как открытый пробел в шапке
(`dom_parser.rs:18-25`, «Phase 1: namespace-aware XML output... Not yet
implemented: ... XML error-document on parse failure for XML MIME types»), но
не называет главный симптом прямо: не «нет специальной обработки ошибок» —
XML вообще не парсится как XML, только как HTML-фрагмент с HTML-специфичным
оборачиванием.

Отдельная, независимая реализация — `document.implementation.createDocument`
(`dom.rs:2409-2429`, натив `_lumen_create_element_ns`) — строит настоящий
корневой элемент с произвольным именем через арена-бэкенд и этим дефектом не
страдает (подтверждено юнит-тестом `create_document_builds_xml_document`,
исправленным в рамках BUG-367). Значит фикс для `DOMParser` — не открытие
нового XML-парсера с нуля, а либо (а) минимальный XML-режим в существующем
токенизаторе `_vParseHTML` (не искать `<html>`, брать первый top-level
элемент как `documentElement` без HTML-обёртки, регистр тегов не приводить к
нижнему при XML MIME — XML case-sensitive, `_vParseHTML` сейчас всегда
`tagN.toLowerCase()`), либо (б) для XML MIME делегировать в
`_lumen_build_detached_document`/`_lumen_create_element_ns`, как уже делает
`createDocument`.

## Impact

Оба неманульных не-XSLT теста категории `xml` бьют именно в этот дефект
(`eol-normalization.html`: 0/3 сабтеста; `xml-prolog-accepted-versions.html`:
номинально 4/10, но все 4 — ложноположительные). `DOMParser` с
`'text/xml'`/`'application/xml'` — стандартный способ парсинга XML/RSS/SOAP
на живых сайтах и в тестах; сейчас он всегда даёт HTML-документ вместо XML.

## Suspected fix direction

В `_vBuildDocument`: при XML-семье MIME (`application/xml`, `text/xml`,
`application/xhtml+xml`, `image/svg+xml`) не идти в ветку
«html/head/body-обёртка» — взять единственный top-level элемент, распарсенный
`_vParseHTML`, как `documentElement` напрямую (без синтетических head/body;
`doc.head`/`doc.body` остаются `null`, как и должно быть для не-HTML
документа), сохранить регистр имени тега как есть (не приводить к нижнему —
`VElement` конструктор сейчас всегда лоуеркейсит `tagName`/`localName`).
Верификация: `run_report.py --all --root xml --recursive`, ожидание —
`documentElement.tagName === 'a'`/`'b'`/… вместо `'HTML'` в
`xml-prolog-accepted-versions.html`, EOL-нормализация в
`eol-normalization.html` проверяема напрямую на реальном дереве.

## Измеренный вес (WPT-VENDOR-xml, 2026-08-18)

Прогон вендоренной категории `xml` (`run_report.py --all --root xml
--recursive`, 56.79 с, 20 id по глобу / 7 фактически исполненных
wptrunner-ом, 3/7 harness OK, 4/190 сабтестов). Остальные 5 исполнившихся id
(`xslt/*`) падают на полностью отсутствующем `XSLTProcessor` (грепом по
`crates/` — не встречается вовсе); XSLT нигде не упомянут как запланированный
скоуп (`CAPABILITIES.md`/`ROADMAP.md`/`CSS-SPECS.md` молчат), это отдельная,
большая legacy-подсистема — новых багов на это не заведено, аналогично
`RTCIceTransport` в `WPT-VENDOR-webrtc-ice`. Один `xslt/fetch/xslt.https.sub.html`
падает `ERROR` на уже задокументированном TLS-гэпе (BUG-438/BUG-657-класс:
«navigate reported success but the document was never replaced»), тоже не
новый номер.

## Фикс (2026-08-23, P1)

Выбран вариант (а) из «Suspected fix direction» — XML-режим в существующем
токенизаторе, а не делегирование в арену. Причина: `DOMParser` строит
**отсоединённый** документ на JS-объектах (`VNode`/`VElement`/`VDocument`,
весь `dom_parser.rs` — самостоятельный мирок), а `_lumen_create_element_ns`
живёт в арене живого документа; путь (б) означал бы либо второй тип
документа в возвращаемом значении `parseFromString`, либо перенос всего
`VNode`-дерева на арену — это не фикс бага, а переписывание подсистемы.

Что сделано (всё в `crates/js/src/dom_parser.rs`):

- `_vParseHTML(html, doc, isXML)` получил XML-режим. Он меняет ровно четыре
  места, где XML расходится с HTML: имена не приводятся к нижнему регистру
  (XML 1.0 §2.3), закрывающий тег обязан совпасть с внутренним открытым
  элементом посимвольно, void-элементов нет (самозакрывается только `<x/>`,
  поэтому `<a><br/></a>` — валидный документ с потомком `br`, а не с
  пустым), и у `<script>`/`<style>` нет raw-text-режима. Нарушение
  well-formedness бросает помеченную ошибку (`__vXmlWF`).
- `_vBuildXMLDocument(xml, mime)` — новая ветка `_vBuildDocument` для четырёх
  XML MIME. Нормализует CRLF и одиночный CR в LF **до** разбора (XML 1.0
  §2.11), валидирует `VersionNum` пролога как `'1.' [0-9]+` (§2.8), берёт
  единственный top-level элемент как `documentElement` без синтеза
  `html`/`head`/`body`; `doc.head`/`doc.body` остаются `null`.
- Фатальная ошибка даёт документ с корнем `<parsererror>` в mozilla-namespace
  (DOM Parsing §8.2), а не исключение — так же, как в остальных движках:
  «два корня», «текст вне корня», «пустой ввод», несовпадение тегов,
  недозакрытый элемент, плохая версия пролога.
- Регистрозависимость протянута через все аксессоры, а не только через
  конструктор: `_vAttrKey` (атрибуты), `cloneNode`, `_vSerializeElement`
  (сериализация круглым ходом отдаёт квалифицированное имя), `_vMatchSimple`
  и `_vGetByTag`. Квалифицированное имя разбирается на `prefix`/`localName`,
  `tagName`/`nodeName` хранят его целиком.

HTML-путь не тронут: `_vParseHTML` без третьего аргумента ведёт себя как
раньше, что закреплено тестом `html_path_still_wraps_and_lowercases`.

Тесты (`crates/js/src/dom_parser.rs`, модуль `tests_v8`, 7 новых):
`xml_document_element_is_the_real_root`, `xml_names_keep_their_case`,
`xml_qualified_name_splits_and_round_trips`, `xml_eol_normalization`,
`xml_prolog_version_is_validated`, `xml_ill_formed_input_yields_parsererror`,
`html_path_still_wraps_and_lowercases`. Оба живых воспроизведения из раздела
«Симптом» покрыты дословно.

### Цифра прогона категории после фикса (2026-08-23)

`run_report.py --all --root xml --recursive` на `dev-release` с фиксом:
**5/10 harness OK, 13/190 сабтестов** против **3/7 harness OK, 4/190** до него.

Знаменатель harness вырос с 7 до 10 не от этого фикса: `run_report.py` с тех
пор стал отбирать и рефтесты (`encoding-single-chunk`, `large-cdata`, `sort`),
а три `xslt/*.window.html` перешли `TIMEOUT` → `ERROR` благодаря фиксу BUG-591
(непойманный `ReferenceError: XSLTProcessor is not defined` теперь всплывает
вместо того, чтобы висеть). Сравнивать надо по сабтестам: знаменатель 190
общий, и все 13 прошедших — это **обе не-XSLT страницы целиком**:

- `eol-normalization.html` — было 0/3, стало 3/3;
- `xml-prolog-accepted-versions.html` — было номинально 4/10 (все четыре —
  ложноположительные, см. «Симптом»), стало 10/10, причём шесть
  «accepted»-сабтестов проходят впервые, а четыре «rejected» теперь верны по
  существу (`tagName === 'parsererror'`, а не `'HTML'`).

Оставшиеся 177 сабтестов — целиком `xslt/`, упираются в отсутствующий
`XSLTProcessor`; потолок категории без XSLT достигнут.

Ожидания WPT сужены (`--update-expected`): два `.ini` с девятью
`expected: FAIL` удалены как ставшие лишними. Гейт `--check` при этом остаётся
красным, но по причинам вне этого бага и вне категории: он сообщает `MISSING`
для десяти id, которые глоб отбирает, а wptrunner никогда не исполняет
(`*-ref.html`, `crashtests/*`, `*-crash.html`), — а контрольный прогон
`--check` на нетронутой категории `webrtc-svc` тоже красный (4 регрессии
`OK` → `ERROR`). То есть baseline TEST-3 просрочен по корпусу в целом;
это территория P2 (`tests/wpt/expectations.py`), отдельной заявки здесь не
заводится.

### Что осталось за скобками

- `namespaceURI` не резолвится из объявлений `xmlns` в области видимости —
  `prefix`/`localName` отделяются, но URI не сопоставляется. Отдельная
  работа: за неё отвечает namespace-aware сериализация, которая шапкой файла
  и так помечена как Phase 1.
- Необъявленная сущность (`&foo;`) остаётся в тексте как есть вместо
  фатальной ошибки — разбора DTD в парсере нет вовсе.
- XSLT (5 из 7 исполнявшихся id категории) этим фиксом не затронут:
  `XSLTProcessor` отсутствует целиком и нигде не запланирован.
