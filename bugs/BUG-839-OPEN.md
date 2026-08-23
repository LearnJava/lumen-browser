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
