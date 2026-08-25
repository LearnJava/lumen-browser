# BUG-839 — `resource`-записи Resource Timing не создаются никогда: тип объявлен в `supportedEntryTypes`, `observe()` его принимает, а хук `_lumen_record_resource_timing` никто не зовёт

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `resource-timing-entry-never-delivered`)
**Область:** `crates/js/src/dom.rs:11600` (`_lumen_record_resource_timing` — вызовы только из юнит-тестов самого шима, `:18097`–`:18175`), `crates/js/src/dom.rs:11459` (`_PERF_SUPPORTED_ENTRY_TYPES` содержит `'resource'`), `crates/js/src/dom.rs:11539` (`_perf_deliver_to_observer` зовёт колбэк двумя аргументами)
**Владелец:** P1/P3 (`lumen-js` + сетевой слой шелла). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
new PerformanceObserver(list => console.log('resource', list.getEntries().length))
    .observe({entryTypes: ['resource']});
fetch('asset.js');                       // запрос уходит и завершается
// колбэк не вызывается никогда
performance.getEntriesByType('resource').length; // 0
```

При этом `PerformanceObserver.supportedEntryTypes` содержит `'resource'`, а
`observe()` тип принимает — то есть страница получает обещание, которое
никогда не исполняется. Это ровно та же форма, что [BUG-809](BUG-809-OPEN.md)
(`layout-shift`): объявленный тип без производителя записей.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 11 тиков):

| вариант | ожидалось | получено |
|---|---|---|
| `po-resource-fetch` | `po-resource resource:…psig-asset.js` | `po-supports …,resource` + `fetch-done` + `po-resource-buffer 0` |
| `po-resource-subresource` | `po-subresource-buffer 3` | `po-subresource-buffer 0 ran=1` |
| `po-callback-options` | `po-args argc=3 options=object`, `po-dropped 0` | `po-args argc=2 options=undefined`, `po-dropped-threw TypeError: Cannot read properties of undefined (reading 'droppedEntriesCount')` |
| `po-navigation-entry` | — | `nav-entries 1 paint=0 all=1` |

Контроль в другую сторону — `mark`/`measure` работают полностью, включая
`buffered: true` (`po-mark-measure` → три записи, `po-buffered` →
`po-buffered-count 3`), и `<link rel=stylesheet>`/`<script src>`/`<img src>`
на той же странице реально загружаются (сервер пробы видит все три запроса,
`ran=1`). То есть дело не в наблюдателе и не в загрузке — записи о ресурсах
просто не рождаются.

## Причина (локализована чтением кода)

Функция-производитель существует и корректна:

```js
// dom.rs:11600
function _lumen_record_resource_timing(url, initiator, start_ms, duration_ms) { … }
```

но `grep` по воркспейсу даёт её вызовы только внутри `#[cfg(test)]`-строк
самого шима (`dom.rs:18097`, `:18110`, `:18127`, `:18144`, `:18161`,
`:18173`) — из `crates/shell` и `crates/network` не зовёт никто. Сетевой слой
завершение загрузки знает (он печатает `← 200 …` и наполняет кеш), но в
Performance Timeline это не попадает.

Второй фасет — сигнатура доставки:

```js
// dom.rs:11539
try { obs._cb(list, obs); } catch(e) {}
```

Performance Timeline L2 §6.2.1 требует три аргумента: `(list, observer,
options)`, где `options.droppedEntriesCount` — число записей, вытесненных из
буфера. Третьего аргумента нет вовсе, поэтому обращение к нему бросает
`TypeError` (а само это исключение съедается — см. [BUG-840](BUG-840-OPEN.md)).

## Масштаб

Маркер `resource-timing-entry-never-delivered` в
`tests/wpt/timeout_audit.py` — **9 id** остатка снимка WPT-RUN-5:
`resource-timing/content-type-minimization`, `event-source-timing`,
`resource_timing_content_length`, `resource_subframe_self_navigation`,
`tentative/stylesheet-initiated`, `performance-timeline/droppedentriescount`,
`po-resource`, `timing-entrytypes-registry/registry`,
`css/fetching/fetch-resources.sub`. Все они ждут колбэка наблюдателя,
поэтому TIMEOUT, а не FAIL.

## Направление починки (не предписание)

Позвать `_lumen_record_resource_timing` из того места шелла, где завершается
загрузка подресурса (там же, где пишется `← 200 …`), с `initiatorType` по
типу инициатора, и добавить третий аргумент `options` в
`_perf_deliver_to_observer`. Ни то, ни другое не требует нового API — обе
стороны контракта уже написаны.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   po-resource-fetch --variant po-resource-subresource --variant
   po-callback-options` — ожидаются `po-resource resource:…`,
   `po-subresource-buffer 3`, `po-args argc=3`.
2. WPT: `run_report.py --all --root resource-timing --recursive` — семейство
   `buffered-flag`/`supported_resource_type` должно перестать быть TIMEOUT.


## Замер 2026-08-23 (WPT-RUN-6, срез 25): пуст не только поток наблюдателя, но и сам буфер

Срез 22 измерил, что `PerformanceObserver` с типом `resource` не получает
записей. Срез 25 отделил вторую половину, которую тесты проверяют чаще:
чтение буфера напрямую, без наблюдателя.
`tests/wpt/verify_focus_mutation_animation_gaps.py --variant perf-resource`
(dev-release, Linux, `main` = `530d0a444`, `--seconds 5`; на странице `<img>` и
`fetch()`, оба запроса сервер пробы **видел**):

| чтение | ожидалось | получено |
|---|---|---|
| `PerformanceObserver({type:'resource', buffered:true})` | 2 записи | колбэк не вызван ни разу |
| `performance.getEntriesByType('resource')` через 400 мс | 2 | **0** |
| то же через 1.4 с (после `rt-fetch-done`) | 2 | **0** |
| `getEntriesByType('mark'/'measure'/'navigation'/'paint')` | 1/1/1/2 | 1/1/1/2 ✔ |
| `'onresourcetimingbufferfull' in performance` | `true` | **false** |
| `clearResourceTimings`/`setResourceTimingBufferSize` | функции | функции ✔ |

Значит дело не в доставке наблюдателю: записи не создаются вообще, а
управляющая обвязка буфера (методы очистки/размера) присутствует и
обманчиво выглядит рабочей. `supportedEntryTypes` продолжает объявлять
`resource` — из-за чего тесты, проверяющие поддержку через список, идут
дальше и виснут вместо честного `FAIL`.

**Масштаб уточнён:** механизм `resource-timing-entries-missing` в
`tests/wpt/timeout_audit.py` — 20 id остатка снимка WPT-RUN-5
(`resource-timing/*` 12, `performance-timeline/*` 5,
`largest-contentful-paint/*` 2, `longtask-timing/supported-longtask-types.window.html`).

## Перезамер 2026-08-23 (WPT-RUN-6, срез 27): буфер пуст для всех инициаторов

`tests/wpt/verify_callback_import_preload_gaps.py --variant resource-timing`
на `main` = `34cbefd25`. Страница делает четыре запроса разных видов —
`<img>`, `fetch()`, `XMLHttpRequest`, `new EventSource()` — сервер пробы
видит все четыре, а буфер Resource Timing остаётся пустым:

```
rt-api getEntriesByType=function setResourceTimingBufferSize=function
       clearResourceTimings=function onresourcetimingbufferfull=false
rt-fetch-done
rt-xhr-done
rt-es-created
rt-entries n=0 []
[server saw: GET /vcip-asset.js, GET /vcip-asset.js?xhr,
             GET /vcip-pixel.png, GET /vcip-sse.py]
```

То есть дело не в `PerformanceObserver` и не в конкретном типе инициатора:
записи не создаются вообще ни для чего. Побочно: у `performance` нет
`onresourcetimingbufferfull` (`in` — `false`), хотя
`setResourceTimingBufferSize`/`clearResourceTimings` есть.

Цена по остатку WPT-RUN-5, сверх прежней: `resource-timing/initiator-type/misc.html`
(ждёт `initiatorType === 'fetch'`), `resource-timing/buffer-full-eventually.html`
(ждёт событие переполнения буфера), `resource-timing/ping-rt-entries.html`
(`observe_entry` по записи типа `ping`).

## Починено 2026-08-25 (P1)

**Статус:** FIXED. Заявка называла один дефект — «функция-производитель есть,
вызывающих нет». Их оказалось **четыре**, и три из них таковы, что запись
некуда положить или нечем прочитать, даже если её создать.

### 1. Записи создаются — на двух путях, а не на одном

Загрузки страницы делятся ровно надвое, и половины лечатся в разных крейтах.

**Что страница начинает сама** — записывается в шиме, там же, где есть
настоящий `performance.now()` вызова. Одна точка (`_perf_rt_record_fetch`
внутри `_lumen_fetch`, `crates/js/src/dom.rs`) покрывает четыре вида
инициаторов: после [BUG-703](BUG-703-FIXED.md)/[BUG-826](BUG-826-FIXED.md)
внешний `<script src>`, стилевой `<link>` и вся семья `rel=preload` грузятся
через тот же `fetch()`. Поэтому `initiatorType` — параметр
(`init._lumenInitiatorType`), а не вывод из URL-а: иначе стилевой файл
отчитался бы как `fetch`. `XMLHttpRequest` правится отдельно — `xhr.rs` это
собственный `rt.eval`, и правка внутри `WEB_API_SHIM` до него не доходит (урок
[BUG-780](BUG-780-FIXED.md)).

**Что движок грузит за страницу** — картинки, стили каскада, тела
`@font-face`, скрипты из разметки. Они идут на рабочих потоках без
JS-контекста, часто до создания рантайма документа. Точка эмиссии —
`HttpClient::fetch_subresource` (`crates/network/src/lib.rs`), единственное
место, через которое проходит любой подресурс движка. **Оба кэша шелла лежат
выше неё** (`PREFETCH_CACHE::fetch_current` заходит внутрь только при
промахе), поэтому спекулятивный прогрев из стримингового потока и финальный
проход не дают двух записей на ресурс. Канал — существующая цепочка
`EventSink` (новое `Event::ResourceTimed`): она уже `Send + Sync` и уже
дотягивается с любого потока; шелл ставит в цепочку `ResourceTimingSink` —
отвод, а не фильтр.

`start_ms` едет как unix-epoch миллисекунды: это тот же отсчёт, из которого шим
берёт `timeOrigin`, поэтому перевод в `DOMHighResTimeStamp` — одно вычитание.
Длительность считается монотонными часами, которые не портит скачок системного
времени. Загрузка, законченная до появления рантайма, зажимается в
`startTime = 0`, а не уезжает в минус.

### 2. Буфер Resource Timing L2 §4.4 — его не было вовсе

`setResourceTimingBufferSize` был пустой заглушкой (`function(_maxSize) {}`), а
`clearResourceTimings` не сбрасывал счётчик. Реализован весь §4.4: лимит (250
по умолчанию), вторичный буфер, флаг ожидания, событие
`resourcetimingbufferfull` и цикл «страница освободила место — то, что должно
было пропасть, всё-таки копируется». Сброс счётчика в `clearResourceTimings` —
не бухгалтерия, а половина операции: это единственный способ, которым страница
освобождает место прямо из обработчика.

Событие ставится **задачей** в `_lumen_timers` с `nesting: 0` (приём
[BUG-832](BUG-832-FIXED.md)), а не диспатчится на месте: обработчик назначают
после переполняющей загрузки не реже, чем до неё.

### 3. `onresourcetimingbufferfull` не существовал как IDL-атрибут

Был обычный экспандо, поэтому `'onresourcetimingbufferfull' in performance`
отвечал `false` — и любая проверка наличия API читала его как отсутствующий,
хотя присваивание «работало». Теперь аксессор на `Performance.prototype`.

### 4. Третий аргумент колбэка

`_perf_deliver_to_observer` зовёт колбэк тремя аргументами (Performance
Timeline L2 §6.2.1). `droppedEntriesCount` присутствует, только пока у
наблюдателя поднят флаг «requires dropped entries»: флаг поднимает **каждый**
`observe()`, опускает первая доставка, отчитавшаяся о счётчике. Счёт идёт
только по `resource` — это единственный буфер движка с границей; у остальных
типов ничего не вытесняется, и ненулевое число там было бы ложью, которую
страница не может проверить.

**Буфер и поток наблюдателя — два независимых стока.** Запись, которой буфер
отказал, всё равно доставляется каждому подписчику: при
`setResourceTimingBufferSize(0)` колбэк входит, а `getEntriesByType('resource')`
пуст. Это ровно то, на чём стоит первый подтест
`performance-timeline/droppedentriescount.any.js`.

### Пятый дефект, найденный замером, а не чтением

Первая версия второй половины не доезжала: страница с `<link>`, `<img>` и
`<script src>` в разметке по-прежнему видела 0 записей, хотя сервер пробы все
три запроса получил. Сброс очереди стоял в `set_js_ctx` — «общей точке всех
путей навигации», — а он вызывается **после** `source.load`, то есть после
того, как документ уже загрузил свои подресурсы. Сброс выбрасывал ровно те
строки, которые страница и должна была получить. Переехал туда, где начинается
загрузка нового документа (блок сброса streaming-состояния, обе копии).

### Шестой дефект: доставка опаздывала на целый документ

Первый прогон WPT после правки показал странное:
`performance-timeline/case-sensitivity.any.html` по-прежнему падает на
`assert_not_equals(lowerList.length, 0, "Resource entries exist")`, хотя проба
на той же машине даёт три записи. Разница — **когда** страница смотрит.

Проба читает буфер из `setTimeout`, то есть после того, как цикл событий
провернулся. WPT-тест читает его **синхронно, первой строкой первого
скрипта**. А подресурсы документа (`testharness.js` в том числе) грузятся во
время парсинга — до создания рантайма, — поэтому подённый дренаж в `Lumen` к
этому моменту ещё ни разу не выполнялся.

Доставка добавлена туда, где рантайм только что создан и скрипты ещё не
запущены (`parse_and_layout`, сразу после `install_dom`). Подённый дренаж
остаётся и покрывает хвост: картинки и всё, что стартовало позже.

Из этого вылез седьмой — уже не наблюдавшийся, но неизбежный: пока новый
документ грузится, активен рантайм **уходящей** страницы, и подённый дренаж
отдал бы её строки нового документа. Очередь теперь на время навигации
приостановлена (`clear()` поднимает флаг, `set_js_ctx` опускает), а
загружающийся документ забирает строки безусловно — он и есть их адресат.

### Замеры (dev-release, Windows, локальный http-сервер пробы)

| проба | было | стало |
|---|---|---|
| `verify_perf_idb_sse_gaps.py --variant po-resource-fetch` | `po-resource-buffer 0` | запись с именем URL-а + `po-resource-buffer 1` |
| то же, `--variant po-resource-subresource` | `po-subresource-buffer 0 ran=1` | `po-subresource-buffer 3 ran=1` |
| то же, `--variant po-callback-options` | `argc=2 options=undefined`, `TypeError` | `argc=3 options=object`, `po-dropped 0` |
| `verify_callback_import_preload_gaps.py --variant resource-timing` | `rt-entries n=0`, `onresourcetimingbufferfull=false` | `rt-entries n=3 [fetch, xmlhttprequest, img]`, `onresourcetimingbufferfull=true` |
| `verify_focus_mutation_animation_gaps.py --variant perf-resource` | колбэк не вызван, `rt-late resource=0` | наблюдатель получил обе записи, `rt-late resource=2` |

**WPT A/B на одной машине, один и тот же корпус** (`run_report.py --all --root
performance-timeline --recursive --limit 38`, dev-release, Windows; предел 38
отсекает подкаталог `not-restored-reasons`, который вешает браузер на
bfcache/iframe и уносит остаток шарда — [BUG-480](BUG-480-OPEN.md)):

| | до | после |
|---|---|---|
| файлов harness OK | 22/38 | **25/38** |
| подтестов PASS | 23/57 | **27/57** |

Перестали быть TIMEOUT: `droppedentriescount.any.html`, `po-observe.html`,
`po-resource.html`. `case-sensitivity.any.html`'s `getEntriesByType values are
case sensitive` FAIL → PASS. Четыре подтеста `droppedentriescount` перешли из
NOTRUN в честный FAIL — файл теперь доходит до конца вместо того, чтобы висеть
на первом.

10 юнит-тестов в `dom.rs` (буфер, вторичный буфер, событие как задача,
обработчик, освобождающий место, `droppedEntriesCount` один раз на `observe()`,
`toJSON`, доставка строк от шелла), 4 в `crates/shell/src/resource_timing.rs`.
Три теста `fetch_subresource_*` в `lumen-network` считали события на sink-е
поштучно (2 → 3) — обновлены и проверяют содержимое новой записи.

### Остаток (не входит в этот баг)

* **`EventSource` записи не даёт.** Запрос делается (сервер пробы его видит),
  но идёт своим долгоживущим соединением (`crates/network/src/sse.rs`), мимо
  `fetch_subresource`. Запись типа `resource` с `initiatorType: 'other'` для
  него — отдельная работа.
* **Неудачная загрузка записи не порождает.** Спека создаёт запись и для
  ответа с ошибкой; шим пишет только успешный путь.
* **Под-тайминги фазовые.** `domainLookupStart`…`requestStart` схлопнуты на
  `fetchStart`: у движка нет по-фазового разбора соединения. `redirectStart`/
  `redirectEnd`/`workerStart` — нули, `nextHopProtocol` пуст для подресурсов
  движка.
* **Доставка наблюдателю по-прежнему синхронная**, а не задачей
  ([BUG-648](BUG-648-OPEN.md)): `buffered-does-not-sync-invoke.html` это
  проверяет отдельно. `droppedEntriesCount` от этого не зависит — флаг
  «requires dropped entries» пер-наблюдательный.
* **`transferSize`** — `encodedBodySize + 300` (фиксированная аппроксимация
  накладных расходов ответа из спеки), а не реальный объём на проводе.
