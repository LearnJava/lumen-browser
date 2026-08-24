# BUG-904 — URL, оканчивающийся на `?`, даёт `search === '?'` вместо `''`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::_lumen_parse_url`, `URL_PARSE_SHIM` — ветка `qIdx >= 0`; то же значение читают `location.search` страницы, `URL.prototype.search` и `WorkerLocation.search`)
**Найден:** P1, 2026-08-24, при закрытии [BUG-776](BUG-776-FIXED.md) — `workers/WorkerLocation_search_empty.htm`

## Симптом

```js
new URL('http://example.test/x?').search   // → '?'   (должно: '')
```

Единственный провал из 19 файлов `workers/WorkerLocation_*.htm`/
`WorkerNavigator_*.htm` после починки BUG-776 (остальные 18 — PASS):

```
FAIL  WorkerLocation.search with empty <query>  - assert_equals: expected "" but got "?"
```

Тест грузит воркер по адресу `WorkerLocation_search_empty.js?` и проверяет,
что `location.search` пуст.

## Причина

`_lumen_parse_url` (`URL_PARSE_SHIM`) режет строку по первому `?` и кладёт
в `search` весь хвост **вместе с вопросительным знаком**, не отличая
«запрос пустой» от «запроса нет»:

```js
var qIdx = rest.indexOf('?');
if (qIdx >= 0) { search = rest.slice(qIdx); rest = rest.slice(0, qIdx); }
```

По URL Standard §6.3 геттер `search` возвращает пустую строку, если query
равен `null` **или** пустой строке, и только иначе — `'?' + query`. То же
правило действует и для `hash` (URL, оканчивающийся на `#`, обязан давать
`hash === ''`) — вторая половина того же дефекта, тем же кодом на две
строки ниже.

## Область

Функция общая, поэтому дефект виден одинаково из трёх мест: `location.search`
страницы, `URL.prototype.search`/`hash` и (с 2026-08-24) `WorkerLocation`.
Чинить надо в одном месте, но проверять — во всех трёх: правка меняет
наблюдаемое значение на странице, а не только в воркере.

Не путать с [BUG-829](BUG-829-OPEN.md): там `location` вообще не
переразбирается после `history.pushState`, здесь — разбор идёт, но
сериализация пустого запроса не по спеке.

## Как проверить

```
tests/wpt/.venv/Scripts/python tests/wpt/run_smoke.py \
  --binary target/dev-release/lumen.exe /workers/WorkerLocation_search_empty.htm
```
(в Git Bash — с `MSYS_NO_PATHCONV=1`, иначе id теста превращается в путь).
Должно стать 1/1 PASS. Плюс юнит-тест на `_lumen_parse_url` в `dom.rs`
на обе ветки (`'?'` и `'#'`).
