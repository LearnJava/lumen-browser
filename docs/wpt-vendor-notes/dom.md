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
