# BUG-777 — `Worker`/`SharedWorker` конструкторы полностью игнорируют аргумент `options`: модульных воркеров не существует

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs:483` — `function Worker(url)`, второй параметр не объявлен; `crates/js/src/shared_worker.rs:302` — `function SharedWorker(url, name)`, второй параметр всегда трактуется как строковое имя)
**Найден:** P2, WPT-VENDOR-workers, 2026-08-18 — `run_report.py --all --root workers --recursive`

## Симптом

`new Worker(url, {type: 'module', ...})` и `new SharedWorker(url, {type:
'module', ...})` создают обычный классический воркер: скрипт исполняется
как non-module, и любой `import`/`export` в его теле немедленно роняет
`SyntaxError`/`Runtime("Cannot use import statement outside a module")`.
В прогоне категории (94/249 harness OK) это — весь каталог
`workers/modules/` (24 файла, целиком TIMEOUT) плюс
`SharedWorker-extendedLifetime-named-module.html`:

```
modules/dedicated-worker-import{,-blob-url,-csp,-data-url,-data-url-cross-origin,
  -failure,-meta,-referrer}*, modules/dedicated-worker-options-{credentials,type}.html,
modules/dedicated-worker-parse-error-failure.html,
modules/shared-worker-import{,-blob-url,-csp,-data-url,-data-url-cross-origin,
  -failure,-meta,-referrer}*, modules/shared-worker-options-{credentials,type}.html,
modules/shared-worker-parse-error-failure.html,
SharedWorker-extendedLifetime-named-module.html
```

Лог: 7 × `Runtime("Cannot use import statement outside a module")`.

## Причина

`worker.rs:483`:

```js
function Worker(url) {
  var script;
  var u = String(url || '');
  ...
```

Второго параметра нет вовсе — `new Worker(url, {type: 'module'})` тихо
отбрасывает второй аргумент, `script` затем передаётся в
`_lumen_create_worker(script)` (`worker.rs:550`) и исполняется как
классический скрипт (`run_worker_thread_v8` не знает о ES-модулях —
`grep -n "type.*module\|is_module" crates/js/src/worker.rs` пуст).

`shared_worker.rs:302`:

```js
function SharedWorker(url, name) {
  var nm = (name === undefined || name === null) ? '' : String(name);
```

Спека (HTML LS §10.1.1, `SharedWorker(scriptURL, options)`) требует
принимать либо строку (имя), либо `WorkerOptions`-словарь
(`{name, type, credentials}`) — здесь любое второе значение сразу
коэрсится в строку через `String(name)`. Передача `{type: 'module'}`
даёт `nm === "[object Object]"`, то есть SharedWorker с этим именем
молча появляется под мусорным идентификатором, а `type` теряется точно
так же, как у `Worker`.

## Почему это не только тестовый шум

Module workers — не экспериментальная фича: `{type: 'module'}` — основной
современный способ подключить воркер, использующий `import`, без сборки в
один файл (webpack/esbuild-воркеры, любой код на нативных ES-модулях).
Сегодня в Lumen единственный работающий путь — вручную инлайнить всё в один
классический скрипт; любая страница, написанная по актуальным гайдам MDN/
web.dev («Worker modules»), не запустится вовсе. `CAPABILITIES.md` не
заявляет module-воркеры — не регресс, а неисследованный пробел (та же
формулировка, что у [BUG-776](BUG-776-FIXED.md), общая природа: раздел
Workers/Concurrency описывает только классический путь).

## Предлагаемый фикс

- Прочитать второй аргумент в обоих конструкторах: если это `string` —
  трактовать как `name` (для `SharedWorker`, обратная совместимость), если
  объект — читать `.type`/`.credentials`/`.name`.
- Прокинуть `type === 'module'` через `_lumen_create_worker`/
  `_lumen_sw_connect` (или отдельный флаг) до `run_worker_thread_v8`, чтобы
  скрипт компилировался/исполнялся как ES-модуль (V8 module API, не
  `Script`) — это, скорее всего, отдельная работа сравнимого объёма с самим
  фиксом остальных полей `WorkerOptions`, поэтому в этом баге фиксируется
  только диагноз, не разбивка на слайсы.
- `credentials`/именование безопасности — по умолчанию `'same-origin'`,
  сегодня не читается вовсе (нет разницы в поведении по этому полю, значит
  не проверяемо прогоном; заметка для будущей реализации).

## Не расследовано в этой сессии

- Точное поведение импорта модулей внутри самого модульного воркера
  (`import` относительных путей, `import.meta.url`, циклические импорты) —
  до появления самого модульного режима непроверяемо.
- `dedicated-worker-options-credentials.html`/`shared-worker-options-
  credentials.html` могут вскрыть отдельный дефект после того, как `type`
  начнёт читаться.

## Перемер WPT-RUN-6, срез 26 (2026-08-23)

`tests/wpt/verify_worker_port_storage_gaps.py --variant worker-type
--variant worker-module-syntax --variant sw-name` на `main` = `c14b8068c`
(dev-release, Linux) — диагноз подтверждён и уточнён тремя фактами,
которых в исходной заявке не было:

1. **`{type: 'module'}` действительно исполняется как классический скрипт**
   — проверено скриптом, тело которого валидно только как модуль:
   `wms-err-module Cannot use import statement outside a module`, и ровно
   та же строка для контрольного запуска того же файла без опций.
2. **Невалидный `type` не бросает `TypeError`.** `dedicated-worker-options-
   type.html` проверяет это двумя `test()`: `new Worker(url, {type: ''})` и
   `{type: 'unknown'}` обязаны бросить синхронно. Измерено:
   `wt-built-empty`, `wt-built-unknown`, затем оба воркера отвечают
   `wt-msg-empty echo:hello` / `wt-msg-unknown echo:hello` — конструирование
   проходит, скрипт запускается.
3. **Строковая и словарная формы имени `SharedWorker` расходятся по разным
   глобалам** — не «имя потеряно», а раскол идентичности:
   `String({name:'my name'})` даёт ключ `name:[object Object]`, поэтому два
   клиента со словарём попали в один воркер, а третий с той же строкой —
   в другой (счётчики подключений 1, 2 и снова 1). Эта часть вынесена
   в [BUG-866](BUG-866-OPEN.md) вместе с двумя соседними дефектами
   идентичности (`self.name` не задаётся, `URLMismatchError` не бросается),
   потому что чинится в той же строке `shared_worker.rs:385`, но не
   сводится к «options не читаются».

Три id остатка снимка WPT-RUN-5 из этого механизма —
`dedicated-worker-options-type.html`, `shared-worker-options-type.html` и
`*.any.sharedworker-module.html` (`dynamic-import/blob-url`,
`import-meta/import-meta-resolve`, `import-meta/import-meta-url`).
