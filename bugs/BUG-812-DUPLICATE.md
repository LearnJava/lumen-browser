# BUG-812 — `XMLHttpRequest.open()` не разрешает относительный URL: запрос уходит в сеть строкой и умирает с `invalid url: missing scheme`

**Статус:** DUPLICATE → [BUG-780](BUG-780-FIXED.md)
**Слит:** 2026-08-22 — тот же дефект, та же строка (`xhr.rs`, `this._url = String(url)` в `open()`), тот же фикс (резолв против базы документа в `open()`). BUG-780 заведён раньше (2026-08-18, WPT-VENDOR-xhr) и потому выживает; **чинить и закрывать нужно его**. Уникальное из этого файла перенесено в BUG-780: живая проба `verify_csp_url_worker_gaps.py` (`--variant xhr-relative`/`xhr-root-relative` с контролями) и корпусной счёт механизма `relative-url-unresolved` (71 id остатка WPT-RUN-5). Файл оставлен целиком: на него ссылаются `timeout_audit.py`-отчёты и заметки среза 18.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 18 — 71 TIMEOUT остатка, механизм `relative-url-unresolved`)
**Область:** `crates/js/src/xhr.rs:231` (`this._url = String(url)` в `open()`), `crates/js/src/xhr.rs:312-314` (`send()` отдаёт `this._url` в `_lumen_fetch_sync_with_body`/`_lumen_fetch_sync` как есть)
**Владелец:** P1/P3 (движок, `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Любой XHR с относительным URL — то есть почти любой XHR в вебе — не
выполняется:

```js
var x = new XMLHttpRequest();
x.open("GET", "resources/status.py");   // ← так пишет весь каталог xhr/
x.send();                                // → onerror, status = 0
```

В stderr браузера:

```
fetch error: invalid url: invalid url: missing scheme: "resources/status.py"
```

Для теста это ожидание `load`/`readystatechange`, которых не будет, то есть
TIMEOUT; для страницы — молчаливый отказ, потому что событие `error` XHR
диспатчит, а причину видно только в логе браузера.

## Прямое измерение

`tests/wpt/verify_csp_url_worker_gaps.py` (живое окно, http, страница
отдаётся тем же сервером, что и цель запроса; 2026-08-21, коммит `41ee56b73`):

| проба | ожидалось | получено |
|---|---|---|
| `xhr-relative` — `open("GET", ".cspgap-target.txt")` | `xhr-load status=200` | `xhr-error status=0` |
| `xhr-root-relative` — `open("GET", "/.cspgap-target.txt")` | `xhr-load status=200` | `xhr-error status=0` |
| `xhr-absolute` — `open("GET", location.origin + "/…")` | `xhr-load status=200` | `xhr-load status=200` ✔ |
| `fetch-relative` — тот же относительный URL через `fetch()` | `fetch-resolved status=200` | `fetch-resolved status=200` ✔ |

Две последние строки — контроль, отделяющий «XHR сломан» и «сеть сломана»
от «не разрешается относительный URL именно в XHR». Абсолютный URL через
XHR работает, относительный через `fetch()` работает; не работает только их
пересечение.

## Причина (локализована чтением кода)

`XMLHttpRequest.prototype.open` (`xhr.rs:231`) кладёт `String(url)` в
`this._url` без разрешения относительно базового URL документа, а `send()`
(`xhr.rs:312-314`) передаёт эту же строку в `_lumen_fetch_sync*`. Дальше её
разбирает `lumen-network`, которому базовый URL неизвестен, — отсюда
`missing scheme`.

Разрешение в движке есть и работает: шим `fetch()` резолвит относительный
URL через базовый URL документа ([BUG-347](BUG-347-FIXED.md)), и тот же
дефект уже чинили один раз для `window.open`
([BUG-359](BUG-359-FIXED.md)). XHR — поверхность, которую тогда пропустили.
По XHR §4.5.1 шаг 6 `open()` обязан разрешать URL относительно
«relevant settings object's API base URL», причём **в момент `open()`**, а
не `send()`.

## Масштаб

Механизм `relative-url-unresolved` забирает **71 id** остатка снимка
WPT-RUN-5 (`html/infrastructure` 15, `html/semantics` 5, `html/webappapis` 3,
`websockets/security` 2, хвост по 1–2 из `custom-elements`, `dom/nodes`,
`domxpath`, `encoding`, `resource-timing`, `navigation-api`).

Счёт занижен вдвойне и это важно при оценке отдачи от фикса:

* механизм стоит **последним** среди сетевых причин в
  `timeout_audit.py` — id, где вместе с этой строкой напечатана более
  решающая (упавший `importScripts`, 404 хелпера), отданы им;
* весь каталог `xhr/` (≈700 id) в снимке WPT-RUN-5 отработал по другим
  причинам (`helper-404` — его хелперы `resources/*.py` требуют
  выполнения python-обработчиков) и в 71 id не входит. После починки
  относительных URL он станет прогоняться по-настоящему.

## Направление починки (не предписание)

Разрешать URL в `open()`, а не в `send()` (спека требует именно этого:
`responseURL`, `abort()` и проверки CORS смотрят на уже разрешённый URL).
Базовый URL брать тем же способом, что и шим `fetch()`
(`_lumen_document_base_url()`/`_lumen_loc_href`), чтобы поведение двух API
совпадало по построению, а не по совпадению. Отдельно проверить
`open()` до навигации и после — база должна браться на момент вызова.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
   --variant xhr-relative --variant xhr-root-relative` — обе печатают
   `xhr-load status=200`.
2. `grep -rn "missing scheme" .tmp/wpt-corpus/*.raw.jsonl` после перепрогона
   каталога `xhr` не даёт совпадений с относительными путями.
3. WPT: `run_report.py --all --root xhr --recursive` — категория начинает
   давать содержательные вердикты вместо сетевых отказов.
