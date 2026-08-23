# WPT vendor notes — `dom`

## Вендоринг (`tests/wpt/VENDOR.md`)

Категория `dom` была единственной вендоренной частично: с 2026-07-13 на диске лежал только
`dom/nodes/` (436 файлов, 168 тестовых — курируемый S5/S6-гейт `run_suite.py`). Остаток
довендорен 2026-08-23 задачей `WPT-VENDOR-dom-rest` (`ROADMAP.md`) с того же запиненного
коммита `35be3b44f3111c4d614b5b201e399493d20e7b38` — sparse-клон по процедуре
`tests/wpt/VENDOR.md`, файлы verbatim, без правок.

Довендорено 361 файл / 4.2 МБ:

| Подкаталог | Файлов | Что внутри |
|---|---:|---|
| `events/` | 190 | Крупнейший: `Event`-интерфейсы, dispatch/propagation, `EventTarget`, `AddEventListenerOptions`, `scrolling/`, `legacy-pre-activation-behavior/` |
| `ranges/` | 61 | `Range`/`StaticRange`/`AbstractRange`, `Range-mutations-*` |
| `traversal/` | 32 | `NodeIterator`, `TreeWalker`, `NodeFilter` |
| `observable/` | 30 | `Observable` (WICG proposal, `tentative`) |
| `collections/` | 12 | `HTMLCollection-*`, `domstringmap-*` |
| `abort/` | 11 | `AbortController`/`AbortSignal`, включая `AbortSignal.any()`/`timeout()` |
| `lists/` | 6 | `DOMTokenList-*` |
| `crashtests/` | 4 | Тип манифеста `crashtest` — исполнителя нет (известный инфра-гэп) |
| корневые файлы | 15 | `historical.html`, `idlharness.any.js`/`.window.js`, `interface-objects.html`, `common.js`, `constants.js`, `META.yml`, … |

Перед копированием остатка `dom/nodes/` побайтово сверен с апстримом (`diff -rq` — пусто),
так что вся категория теперь verbatim-апстрим: 797 файлов, ровно как в пине.

### Внекатегорийные зависимости

`find_missing_resources.py --root dom --ids` после вендоринга нашёл 8 отсутствующих путей;
6 довендорены с того же пина (строка инфраструктуры в `tests/wpt/VENDOR.md`):

* `/common/reftest-wait.js` (3 файла, 2 id) — стандартный reftest-хелпер «держи страницу
  до готовности»; числился в общем бэклоге `WPT-RUN-11` — **RUN-11 может вычесть его
  из своего списка**, как и `/common/blank.html` (1 файл).
* `/common/dummy.xhtml`, `/common/dummy.xml` (по 2 файла) — XML-фикстуры для
  `DOMImplementation`/`XMLDocument`-тестов.
* `/common/redirect.py` — `wptserve`-хендлер, через который `dom/nodes/Document-URL.html`
  проверяет URL после редиректа.
* `/resources/blank.html` — грузится в iframe из `dom/nodes/moveBefore/iframe-document-preserve.window.js`.

Два оставшихся не вендорятся сознательно:

* `/common/canvas-frame.css` (1 id) — `Node-cloneNode-external-stylesheet-no-bc.sub.html`
  строит этот URL, но такого файла в апстриме на пине **нет вовсе** (апстримная копия лежит
  по другому пути, `html/canvas/resources/canvas-frame.css`); это апстримный дефект теста,
  а не дыра вендоринга.
* `/resources/WebIDLParser.js` (0 id) — ложное срабатывание сканера: `tools/serve/serve.py::rewrites`
  переписывает этот путь на вендоренный `resources/webidl2/lib/webidl2.js`, сканер не
  моделирует rewrites.

`tests/wpt/dom/LICENSE-WPT.md` добавлен по общему правилу «каждый вендоренный верхнеуровневый
каталог несёт свою копию 3-Clause BSD» (у `dom/` его не было, пока категория была частичной).

## Прогон 2026-08-23 (`docs/wpt-status.md`)

```
LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root dom --recursive --processes=4 \
  --log-raw .tmp/wpt-dom-raw.jsonl --out .tmp/wpt-report-dom-rest.html
```

Linux, `dev-release`, пин `35be3b44`, 5 мин 07 с на 4 процесса. Отобран 641 id,
до `test_end` дошло 608 (33 не попали в очередь — `crashtest`/reftest-типы, для
которых у минимального BiDi-исполнителя реализации нет, тот же известный
инфра-гэп, что у `cssom`/`acid`).

**Итог: 462/608 harness OK, 2572/7152 сабтестов PASS.**

Разделение на «уже вендоренное» и «довендоренное этой задачей»:

| Часть | id | OK | TIMEOUT | ERROR | сабтесты |
|---|---:|---:|---:|---:|---|
| `dom/nodes/` (курируемая, вендорена 2026-07-13) | 281 | 221 | 49 | 8 | 2026/5573 |
| **остаток `dom/` (эта задача)** | **327** | **241** | **22** | **63** | **546/1579** |

По подкаталогам довендоренной части:

| Подкаталог | id | OK | TIMEOUT | ERROR | сабтесты | Головная причина |
|---|---:|---:|---:|---:|---|---|
| `events/` | 192 | 141 | 16 | 35 | 198/578 | россыпь известных гэпов (см. ниже), крупнейший локальный — [BUG-865](../../bugs/BUG-865-OPEN.md) |
| `ranges/` | 57 | 33 | 0 | 24 | 10/251 | [BUG-863](../../bugs/BUG-863-OPEN.md) — **все 24 ERROR** из одной причины |
| `observable/` | 29 | 25 | 4 | 0 | 0/251 | `Observable` не реализован (WICG-предложение, каталог `tentative/`) |
| `traversal/` | 18 | 14 | 1 | 3 | 26/56 | 3 ERROR — та же [BUG-863](../../bugs/BUG-863-OPEN.md) |
| `collections/` | 10 | 10 | 0 | 0 | 11/53 | `HTMLCollection.namedItem` отсутствует |
| `abort/` | 6 | 6 | 0 | 0 | 33/37 | практически зелено |
| `lists/` | 5 | 4 | 0 | 1 | 123/189 | `DOMTokenList` живой; ERROR — дубли имён сабтестов в одном файле |
| корневые `dom/*.html` | 10 | 8 | 1 | 0 | 145/164 | — |

### Заведено (новые причины)

* [BUG-863](../../bugs/BUG-863-OPEN.md) — `document.createCDATASection` отсутствует.
  **Крупнейшая единичная причина в категории: 31 id из 86 не-OK.** Вызов стоит в
  общем `setupRangeTests()` (`tests/wpt/dom/common.js:60`), то есть в `setup()`
  каждого теста `Range`/`NodeIterator`/`TreeWalker` — исключение летит до
  регистрации первого `test()`, и файл отдаёт `ERROR` вместо списка сабтестов.
  Отсюда `ranges/` 10/251: категория не «провалена», а не запущена.
* [BUG-864](../../bugs/BUG-864-OPEN.md) — `Node.lookupNamespaceURI`/`lookupPrefix`/
  `isDefaultNamespace` отсутствуют целиком (70 сабтестов одного файла, ни одного PASS).
* [BUG-865](../../bugs/BUG-865-OPEN.md) — опция `passive` у `addEventListener` не
  разбирается: 57 FAIL в `passive-by-default.html`, весь
  `AddEventListenerOptions-passive.any.*` и 6 файлов `non-cancelable-when-passive/`.
  Тихий дефект: страница просит пассивный слушатель, получает обычный.

### Переподтверждено (существующие баги, новых не заводилось)

| Баг | Что видно в прогоне `dom` |
|---|---|
| [BUG-590](../../bugs/BUG-590-OPEN.md) `document.createEvent` | 69 сабтестов + 2 harness ERROR (`Event-constants`, `Event-propagation`, `EventTarget-dispatchEvent`, `Event-initEvent`, `Event-cancelBubble`) |
| [BUG-577](../../bugs/BUG-577-OPEN.md) `Event.composedPath()` | 2 harness ERROR (`EventTarget-constructible.any`, `window-composed-path`) |
| [BUG-478](../../bugs/BUG-478-OPEN.md) `Element.getClientRects` | `Event-dispatch-redispatch`, `scrolling/input-text-scroll-event-when-using-arrow-keys` |
| [BUG-482](../../bugs/BUG-482-OPEN.md) `document.scrollingElement` | почти весь `events/scrolling/` — 13 ERROR вида `Cannot read properties of undefined (reading 'scrollTo'/'scrollLeft'/'style')` |
| [BUG-746](../../bugs/BUG-746-OPEN.md) `document.styleSheets` | 4 ERROR `webkit-{animation-*,transition-end}-event` (`styleSheets[0].insertRule`) |
| [BUG-533](../../bugs/BUG-533-OPEN.md) `StaticRange` | 14 FAIL `StaticRange is not defined` |
| [BUG-689](../../bugs/BUG-689-OPEN.md) `Attr`-подсистема | 32 FAIL `document.createAttribute is not a function` |
| [BUG-480](../../bugs/BUG-480-OPEN.md) вложенные browsing context | `handler-count.html` (3 id, `Browsing context for element was detached`), `scrollend-event-fires-to-iframe-window` |

### Вне скоупа движка, багов не заводилось

* `dom/observable/tentative/` (29 id, 0/251 сабтестов) — `Observable` это WICG-предложение
  в статусе `tentative`, не спека; отсутствие реализации ожидаемо, багом не оформляется.
* `dom/ranges/tentative/OpaqueRange-*` (94 FAIL `*.createValueRange is not a function`) —
  тот же случай: предложение, метода в спеке DOM нет.
* `dom/crashtests/` (4 файла) — тип манифеста `crashtest`, исполнителя нет
  (известный инфра-гэп, как в `cssom`/`acid`).

### Что осталось несведённым

22 TIMEOUT в довендоренной части не разбирались поштучно — по методике
`docs/wpt-status.md` («не одна задача на тест») это следующий срез, а не часть
вендоринга: механику классификации даёт `tests/wpt/timeout_audit.py`, и, судя по
первым строкам лога, минимум часть из них — уже описанный в `CLAUDE.md` сценарий
«одна зависшая страница уносит остаток шарда». Первый проход категории фиксирует
факты; движковые баги из этой задачи не чинятся.
