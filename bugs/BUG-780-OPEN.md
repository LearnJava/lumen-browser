# BUG-780: `XMLHttpRequest.open()` никогда не резолвит относительный URL против document base — шестой независимый сайт семейства BUG-346/347/359/362/370

**Статус:** OPEN

**Компонент:** js (`crates/js/src/xhr.rs:216-236`, `XMLHttpRequest.prototype.open`)

**Найден:** P2, WPT-VENDOR-xhr 2026-08-18 (`run_report.py --all --root xhr --recursive`, 14 мин 19 с, 236/345 harness OK, 157/1244 сабтестов)

## Симптом

`new XMLHttpRequest().open('GET', url)` с относительным (`resources/foo.json`)
или корне-относительным (`/common/blank.html`) URL сохраняет строку как есть
и передаёт её нативным биндингам без изменений — `send()` падает
`invalid url: missing scheme`. Живая проба (`--mcp-live-port`, страница
`http://127.0.0.1:18471/xhr_probe.html`) подтверждает напрямую:

```
XHR open() relative URL, ._url after open: {"url_after_open":"resources/foo.json"}
XHR open() absolute-path URL, ._url after open: {"url_after_open":"/xhr_probe.html"}
fetch/Request relative URL resolution (уже исправленный сосед):
  {"fetch_resolved_url":"http://127.0.0.1:18471/resources/foo.json"}
```

`fetch()`/`Request` на той же странице резолвят корректно (BUG-347/BUG-370,
исправлены 2026-08-06/2026-08-10) — только `XMLHttpRequest.open()` пропущен.

## Root cause

`XMLHttpRequest.prototype.open` (`crates/js/src/xhr.rs:216-236`) берёт `url`
буквально:

```js
this._url = String(url);
```

— без единого шага резолюции против document base, в отличие от `fetch()`
(`dom.rs`, `WEB_API_SHIM`), которая после фикса BUG-347 пропускает `url`
через `_url_resolve(url, _lumen_document_base_url())` перед вызовом нативных
биндингов. `send()` (`xhr.rs:252+`) передаёт `self._url` как есть в те же
нативные `fetch`-биндинги, что и `fetch()` — значит `Url::parse` в
`crates/network/src/lib.rs` получает нерезолвленную строку и падает по той
же причине, что описана в BUG-347's root cause.

`xhr.rs` — отдельный модуль (`XHR_SHIM`, свой `rt.eval`), не часть
`WEB_API_SHIM` в `dom.rs`, куда попал фикс BUG-347 — по всей видимости
поэтому шестой сайт того же семейства не унаследовал фикс автоматически
(независимая копия проблемы, не регрессия).

## Impact

739 строк `fetch error: invalid url: invalid url: missing scheme: "..."` за
один прогон категории `xhr` (345 id) — доминирующий класс отказов, на
голову выше следующего по весу (`unsupported scheme: data`, 17 строк,
отдельный и заведомый гэп — `data:` URL не поддерживается сетевым слоем
вовсе, `crates/network/src/lib.rs:267`, не путать с этим багом). Затрагивает
любой реальный сайт, использующий `XMLHttpRequest` с относительным URL —
крайне распространённый паттерн, аналогично оценке impact у BUG-347.

Примеры из лога: `"resources/well-formed.xml"`, `"resources/utf16-bom.json"`,
`"resources/delay.py?ms=1000"`, `"/common/blank.html?pipe=trickle(d1)"` — весь
спектр форм (script-relative и root-relative).

## Suspected fix direction

Тот же приём, что BUG-347: в `XMLHttpRequest.prototype.open` резолвить
`url` через уже существующий `_url_resolve(String(url),
_lumen_document_base_url())` перед записью в `this._url` (обе функции уже
определены в `dom.rs` и доступны глобально к моменту `rt.eval(XHR_SHIM)` —
`WEB_API_SHIM` устанавливается раньше `XHR_SHIM` по порядку install).
Верификация: `run_report.py --all --root xhr --recursive`, ожидание —
исчезновение строк `missing scheme` (было 739) и рост `236/345 harness OK` /
`157/1244 сабтеста`.

## Измеренный вес (WPT-VENDOR-xhr, 2026-08-18)

Прогон вендоренной категории `xhr` (`run_report.py --all --root xhr
--recursive`, 14 мин 19 с, 236/345 harness OK, 157/1244 сабтестов) —
предикторы дешевизны (0 `name="variant"`, 1 файл с `testdriver`, 0
`.https.`) подтвердились по стоимости, но не по выходу сигнала: 69
уникальных TIMEOUT-файлов, подавляющее большинство падает именно на этом
баге. Часть оставшихся TIMEOUT — уже известные гэпы: `open-url-multi-window*`
/ `open-url-worker*` (второй барьер BUG-359, `window.open` без реального
`opener`), `xmlhttprequest-timeout-reused.html` и соседи (переиспользуемый
browsing context, класс BUG-380), `json.any.html` (частично `data:` URL, см.
Impact выше).
