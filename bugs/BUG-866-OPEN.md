# BUG-866 — идентичность `SharedWorker` держится только на имени: `self.name` не задаётся, `URLMismatchError` не бросается, а словарь `{name}` уходит в ключ строкой `[object Object]`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `sw-name`)
**Область:** `crates/js/src/shared_worker.rs:382`–`386` — `function SharedWorker(url, name)`: `var nm = String(name)`, `var key = nm ? ('name:' + nm) : ('url:' + String(url || ''))`; `SHARED_WORKER_GLOBAL_SHIM` (там же, `:121`+) не заводит `globalThis.name` вовсе — в отличие от dedicated-воркера, где `worker.rs:345` пишет `globalThis.name = 'worker-' + wid`
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Три отдельных расхождения с HTML LS §10.1.1 в одной строке кода:

1. **`self.name` в `SharedWorkerGlobalScope` — `undefined`.** Имя, с которым
   воркер сконструирован, никуда не передаётся: скрипт не может себя назвать.
2. **Ключ идентичности игнорирует URL.** Спека требует бросить
   `DOMException` `URLMismatchError`, когда с тем же именем приходит другой
   скрипт; здесь второй URL молча подключается к уже живому воркеру первого.
3. **`{name: "..."}` коэрсится через `String()`** (это же записано в
   [BUG-777](BUG-777-OPEN.md) как часть «options игнорируются»), поэтому
   ключ становится `name:[object Object]`. Практическое следствие сильнее,
   чем «имя потеряно»: два клиента со словарём попадают в один глобал, а
   третий с той же строкой-именем — в **другой**, то есть словарная и
   строковая формы конструктора расходятся по разным воркерам.

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant sw-name`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c`, `--seconds 6`).
Страница строит три `SharedWorker` на один скрипт — два с `{name: "my name"}`,
один со строкой `"my name"` — и каждый шлёт счётчик подключений своего
глобала:

```
swn-msg-a {"counter":1,"script":"worker1"}
swn-msg-b {"counter":2,"script":"worker1"}
swn-msg-c {"counter":1,"script":"worker1"}
swn-mismatch-no-throw constructed
```

Читается так: `a`/`b` разделили один глобал (счётчик 1→2), `c` попал в
свой собственный (снова 1) — при том, что по спеке все три обязаны быть
одним воркером со счётчиком 1, 2, 3. Поля `name` в ответах нет вовсе:
`JSON.stringify` выбросил его, потому что `self.name === undefined`.
Последняя строка — конструирование **другого** скрипта под тем же именем:
исключения нет, объект построен.

## Масштаб

7 id остатка снимка WPT-RUN-5 в `workers/`, каждый — свой facet:
`shared-worker-name-via-options.html` (все три формы имени),
`interfaces/SharedWorkerGlobalScope/name/getting.html` (3 подтеста —
`getting name`, `getting name 1`, `getting name 2`),
`constructors/SharedWorker/URLMismatchError.htm`,
`shared-worker-options-type.html`, `semantics/encodings/004.html`,
`SharedWorker_blobUrl.html`, `interfaces/WorkerGlobalScope/location/
redirect-sharedworker.html`. Тесты не падают, а зависают: имя читается
из `onconnect`, который в невыясненный глобал просто не приходит.

## Направление починки (не предписание)

Разобрать второй аргумент по WebIDL (общая работа с BUG-777: строка → имя,
объект → `{name, type, credentials}`), прокинуть имя в
`SHARED_WORKER_GLOBAL_SHIM` тем же способом, что `worker.rs` прокидывает
`wid` (форматирование шима), и сделать ключом идентичности пару
(resolved URL, name), бросая `URLMismatchError`, когда имя совпало, а URL нет.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant sw-name` — ожидается `counter` 1, 2, 3 с `name:"my name"` во
   всех трёх ответах и `swn-mismatch-throws URLMismatchError`.
2. WPT: `run_report.py --all --root workers/constructors/SharedWorker --recursive`.
