# BUG-780: `XMLHttpRequest.open()` никогда не резолвит относительный URL против document base — шестой независимый сайт семейства BUG-346/347/359/362/370

**Статус:** FIXED 2026-08-23 (P1)

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

## Перенесено из BUG-812 (слит как дубликат 2026-08-22)

Тот же дефект был заведён повторно 2026-08-21 (WPT-RUN-6, срез 18) как
[BUG-812](BUG-812-DUPLICATE.md) — независимое подтверждение той же строки и
того же направления починки. Уникальное из него:

**Живая проба с контролями** — `tests/wpt/verify_csp_url_worker_gaps.py`
(живое окно, http, страница отдаётся тем же сервером, что и цель запроса;
коммит `41ee56b73`):

| проба | ожидалось | получено |
|---|---|---|
| `xhr-relative` — `open("GET", ".cspgap-target.txt")` | `status=200` | `xhr-error status=0` |
| `xhr-root-relative` — `open("GET", "/.cspgap-target.txt")` | `status=200` | `xhr-error status=0` |
| `xhr-absolute` — `open("GET", location.origin + "/…")` | `status=200` | `status=200` ✔ |
| `fetch-relative` — тот же URL через `fetch()` | `status=200` | `status=200` ✔ |

Две последние строки — контроль, отделяющий «XHR сломан»/«сеть сломана» от
«не резолвится относительный URL именно в XHR».

**Корпусной счёт.** Механизм `relative-url-unresolved` (`timeout_audit.py`)
забирает **71 id** остатка снимка WPT-RUN-5 (`html/infrastructure` 15,
`html/semantics` 5, `html/webappapis` 3, `websockets/security` 2, хвост по
1–2 из `custom-elements`, `dom/nodes`, `domxpath`, `encoding`,
`resource-timing`, `navigation-api`). Счёт занижен дважды: механизм стоит
последним среди сетевых причин (id, где напечатана более решающая строка,
отданы ей), а весь каталог `xhr/` (≈700 id) в том снимке отвалился раньше по
`helper-404` и в 71 id не входит.

**Уточнение по спеке.** XHR §4.5.1 шаг 6 требует резолвить URL **в момент
`open()`**, а не `send()`: на уже разрешённый URL смотрят `responseURL`,
`abort()` и проверки CORS. Базу брать на момент вызова (проверить `open()`
до и после навигации).

**Как проверить фикс** (в дополнение к прогону категории): 
`tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
--variant xhr-relative --variant xhr-root-relative` — обе печатают
`xhr-load status=200`.

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

## Исправление (P1, 2026-08-23)

`XMLHttpRequest.prototype.open` (`crates/js/src/xhr.rs`) резолвит URL ровно тем
же приёмом, что и `fetch()` после BUG-347:

```js
this._url = (typeof _url_resolve === 'function' &&
             typeof _lumen_document_base_url === 'function')
          ? _url_resolve(String(url), _lumen_document_base_url())
          : String(url);
```

Три решения, которые не сводятся к «дописать вызов»:

- **Резолв в `open()`, а не в `send()`** — XHR §4.5.1 шаг 6. На уже разрешённый
  URL смотрят `responseURL`, `abort()` и проверки CORS, а база должна быть той,
  что действовала в момент `open()`. Тест
  `xhr_open_uses_base_element_in_force_at_open_time` фиксирует обе половины:
  `<base href>`, добавленный после `open()`, не сдвигает уже открытый запрос, а
  следующий `open()` его уже видит. Резолв в `send()` прошёл бы прогон
  категории так же и был бы неверен именно здесь.
- **`typeof`-гард, а не прямой вызов.** `XHR_SHIM` — отдельный `rt.eval`, и оба
  хелпера приходят из `WEB_API_SHIM`, который ставится раньше. Но `xhr.rs`
  вычисляется и в контексте без DOM (свои юнит-тесты, воркерный путь до
  BUG-778), где этих глобалей нет — без гарда шим падал бы на установке, а не
  деградировал к прежнему поведению.
- **Ни строки в `send()`.** Нативные биндинги уже получают `this._url`; менять
  сетевой слой не потребовалось — дефект целиком в одной присваивающей строке.

**Верификация**

1. `cargo test -p lumen-js --features v8-backend xhr::` — 21/21, из них четыре
   новых (относительный, корне-относительный, абсолютный не трогается, база на
   момент `open()`).
2. Живая проба BUG-812 (`tests/wpt/verify_csp_url_worker_gaps.py --variant
   xhr-relative --variant xhr-root-relative --variant xhr-absolute --variant
   fetch-relative`) — все четыре печатают `status=200`; до фикса первые две
   давали `xhr-error status=0` при зелёных контролях:

   | проба | до | после |
   |---|---|---|
   | `xhr-relative` | `xhr-error status=0` | `xhr-load status=200 text=probe-body` |
   | `xhr-root-relative` | `xhr-error status=0` | `xhr-load status=200` |
   | `xhr-absolute` (контроль) | `status=200` | `status=200` |
   | `fetch-relative` (контроль) | `status=200` | `status=200` |

3. **Прогон вендоренной категории после фикса** (`run_report.py --all --root
   xhr --recursive`, тот же скрипт и та же машина, что и замер 2026-08-18):

   | | до (2026-08-18) | после (2026-08-23) |
   |---|---|---|
   | строк `missing scheme` | **739** | **0** |
   | harness OK | 236/345 | **251/345** |
   | сабтестов PASS | 157/1244 | **425/1259** |
   | уникальных TIMEOUT-файлов | 69 | **44** |
   | время прогона | 14 мин 19 с | 12 мин 10 с |

   Сабтесты выросли в 2.7 раза — цифра больше прироста harness OK, потому что
   раньше файл чаще всего доходил до первого запроса и умирал целиком; теперь
   он доходит до конца и падает уже на своих настоящих гэпах. Знаменатель
   вырос (1244 → 1259) по той же причине: часть сабтестов вообще не
   регистрировалась.

   17 строк `unsupported scheme: data` остались — отдельный заведомый гэп
   (`data:` URL не поддерживается сетевым слоем, `crates/network/src/lib.rs`),
   не этот баг. Оставшиеся 44 TIMEOUT — уже известные классы из раздела
   «Измеренный вес»: `window.open` без реального `opener` (BUG-359),
   переиспользуемый browsing context (BUG-380), `<iframe>` (BUG-480).

   Оговорка о чистоте замера: первые ~1.5 мин прогона машина делила CPU с
   `clippy -p lumen-js` (21 с) и `cargo test -p lumen-js -j 2` (67 с). Это
   работает против фикса (лишние TIMEOUT), а не за него, и не влияет на
   счётчик `missing scheme`.
