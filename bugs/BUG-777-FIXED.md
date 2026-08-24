# BUG-777 — `Worker`/`SharedWorker` конструкторы полностью игнорируют аргумент `options`: модульных воркеров не существует

**Статус:** FIXED 2026-08-24
**Компонент:** js (`crates/js/src/worker.rs` — `WORKER_OPTIONS_SHIM`, `WORKER_SHIM`, `spawn_worker_v8`/`run_worker_thread_v8`/`install_worker_globals_v8`; `crates/js/src/shared_worker.rs` — `SHARED_WORKER_SHIM`, `connect_shared_worker_v8`/`run_shared_worker_thread_v8`; `crates/js/src/dom.rs` — `WORKER_LOCATION_NAVIGATOR_SHIM`; `crates/js/src/v8_runtime.rs` — `V8JsRuntime::set_module_context`)
**Найден:** P2, WPT-VENDOR-workers, 2026-08-18 — `run_report.py --all --root workers --recursive`

## Симптом

`new Worker(url, {type: 'module'})` и `new SharedWorker(url, {type:
'module'})` создавали обычный классический воркер: скрипт исполнялся
как non-module, и любой `import`/`export` в его теле немедленно ронял
`Runtime("Cannot use import statement outside a module")`. Весь каталог
`workers/modules/` (22 id) — TIMEOUT либо FAIL, плюс
`SharedWorker-extendedLifetime-named-module.html`.

Второй, независимый симптом того же корня: невалидный `type`
(`{type: ''}`, `{type: 'unknown'}`) обязан бросать `TypeError`
**синхронно**, а конструирование проходило молча.

Третий: у `SharedWorker` любое второе значение коэрсилось в строку-имя
(`String(name)`), поэтому `{name: 'my name'}` открывал воркер под
идентичностью `name:[object Object]` — два клиента со словарём попадали в
один воркер, третий с той же строкой в другой.

## Причина

`worker.rs`: `function Worker(url)` — второго параметра не было вовсе.
`shared_worker.rs`: `function SharedWorker(url, name)` — второй параметр
безусловно шёл в `String(name)`. Ни один из двух путей порождения воркера
(`_lumen_create_worker` → `spawn_worker_v8` → `run_worker_thread_v8`;
`_lumen_sw_connect` → `connect_shared_worker_v8` →
`run_shared_worker_thread_v8`) не знал о существовании модульного режима,
и оба заканчивались `rt.eval(&script)` — компиляцией классического
скрипта.

## Что сделано

**1. Разбор `WorkerOptions` — один общий кусок.** Новый
`worker.rs::WORKER_OPTIONS_SHIM` определяет
`_lumen_parse_worker_options` (WebIDL-конверсия словаря: `undefined`/`null`
→ все умолчания, не-объект → `TypeError`, член со значением `undefined`
считается отсутствующим, `type`/`credentials` — enum-проверка с
`TypeError` на промах) и `_lumen_parse_shared_worker_options` (union
`(DOMString or WorkerOptions)`: строка/число/булево → имя, остальное →
словарь). Кусок вычисляется **обоими** установщиками
(`install_worker_bindings_v8` и `install_shared_worker_bindings_v8`) и
охраняет себя проверкой собственной глобали, поэтому второе вычисление —
no-op.

Почему не одна копия в `WORKER_SHIM`: `SHARED_WORKER_SHIM` — отдельный
`rt.eval`, и правка в первом до второго не доходит (урок
[BUG-780](BUG-780-FIXED.md)); вдобавок юнит-тесты shared-воркера ставят
свои привязки **без** привязок dedicated-воркера, так что ссылка на
чужую глобаль там была бы `ReferenceError`.

Конверсия стоит **первой строкой** конструктора — до резолва URL и до
сетевого похода за скриптом: слой связывания WebIDL преобразует аргументы
раньше, чем тело конструктора что-либо делает, и
`dedicated-worker-options-type.html` проверяет это дважды.

**2. Модульный режим доносится до потока воркера.** `is_module` —
новый аргумент нативов `_lumen_create_worker`/`_lumen_sw_connect` и всей
цепочки порождения. В потоке воркера:

```rust
let outcome = if is_module {
    rt.set_module_context(&script_url, fp_esm);
    rt.eval_module_at(&script_url, &script)
} else {
    rt.eval(&script).map(|_| ())
};
```

`V8JsRuntime::set_module_context` — новый метод (`v8_runtime.rs`): пишет
`v8_esm::set_page_url` + `set_fetch_provider` на **собственном JS-потоке
рантайма**. Он нужен именно потому, что воркерный рантайм никогда не
проходит через `install_dom` (где страница делает те же две записи), а
состояние загрузчика модулей thread-local: резолв-колбэк V8 бескаптурный
и добраться больше никуда не может. Без него `import` в воркере резолвился
бы против пустой базы и не нашёл бы сетевого моста.

`eval_module_at` (а не `eval_module`) — потому что относительный
`import './dep.js'` обязан резолвиться против URL **самого воркера**, а не
страницы; это та же причина, по которой метод существует для внешнего
`<script type=module src=…>`.

Шимы глобальной области — классические скрипты, вычисляемые до тела
модуля, поэтому всё, что они кладут в `globalThis`, модулю видно.

**3. `importScripts()` в модульном воркере бросает `TypeError`.**
HTML LS §10.2.3: у такой области единственный способ подтянуть код —
модульный граф. Проверка стоит до разбора аргументов (бросает и на нулевом
числе аргументов), гейт — новая глобаль `_lumen_worker_is_module`,
выставляемая рядом с `_lumen_worker_base_url`/`_lumen_worker_location_url`
в обоих видах воркера.

**4. Интерфейсы глобальной области воркера.** Без них измерить пункты
1–2 было нечем: ресурсы WPT поголовно ветвятся строкой
`'DedicatedWorkerGlobalScope' in self && self instanceof
DedicatedWorkerGlobalScope`, а область, отвечающая на неё `false`, не
делает вообще ничего — именно поэтому в базовом замере
`dedicated-worker-options-type.html` не проходил даже подтест **классического**
типа. `WORKER_LOCATION_NAVIGATOR_SHIM` (общий для всех трёх видов воркера)
получил `WorkerGlobalScope` и фабрику `_lumen_define_worker_scope(name)`;
каждый вид зовёт её со своим именем — вид отсюда неизвестен, и область не
имеет права выдавать себя за одну из двух других.

`instanceof` несёт цепочка прототипов глобального объекта, а не трюк с
`Symbol.hasInstance`: так методы `EventTarget.prototype` доходят до
области ровно как предписывает HTML LS, а собственные глобали шимов
(`globalThis.x = …`) не задеты — собственное свойство затеняет цепочку.

**5. Ключ идентичности `SharedWorker`.** Имя берётся из словаря, и в
ключ добавлен тип: классический и модульный воркер — разные глобальные
области, и переиспользование одного под другой запустило бы скрипт
второго клиента не в том режиме. Побочно это закрывает раскол
идентичности `name:[object Object]`, отданный в
[BUG-866](BUG-866-OPEN.md) (оставшиеся две трети того бага —
`self.name` и `URLMismatchError` — не тронуты).

**6. Отказ верхнего уровня shared-воркера доходит до первого клиента.**
Найдено измерением, а не заявкой. Поток shared-воркера порождается
**первым** подключением, поэтому `rt.eval` тела скрипта всегда
отрабатывает до того, как придёт хоть один `Connect`, и
`broadcast_shared_worker_error` рассылал отчёт в пустую карту — страница
ждала до внешнего таймаута. У модульного воркера это становится обычным
случаем: каждый провалившийся `import` графа приходит сюда. Ошибка теперь
запоминается (`pending_error`) и повторяется каждому подключающемуся
клиенту. На прогоне это и превращает половину остатка
`shared-worker-*` из TIMEOUT в быстрый отказ, после которого файл
доигрывает остальные подтесты.

## Замеры

Живой A/B, `run_report.py --all --root workers/modules --recursive`,
dev-release, один и тот же слот:

| | harness OK | сабтесты | время |
|---|---|---|---|
| до | 3/22 | 2/160 | 6:25 |
| после | **16/22** | **56/160** | **4:23** |

`dedicated-worker-options-type.html` и `shared-worker-options-type.html` —
**5/5 подтестов каждый** (было 0/5 и 0/5: три promise_test висели на
отсутствующем `DedicatedWorkerGlobalScope`, два `test()` падали на
непроброшенном `TypeError`).

Юнит-тесты: 9 новых (`worker.rs` — 6, `shared_worker.rs` — 3),
в том числе сквозной модульный воркер со статическим импортом через
сетевой дубль и его **отрицательная пара** (тот же скрипт классическим
воркером обязан упасть на `import statement`) — без неё тест был бы зелёным
и при полностью проигнорированной опции.

## Остаток (не в этом баге)

- **`credentials`** читается и проверяется, но на поведение не влияет:
  запросы движка не носят ни `Referer`, ни `Origin`, ни куки-политики
  ([BUG-859](BUG-859-OPEN.md)), поэтому разницы между `omit` и
  `include` пока не существует. Оба `*-options-credentials.html`
  упираются в это, а стартуют вообще с `www1.127.0.0.1` (см. ниже).
- **Кросс-ориджин остаток прогона — не про воркеры.** Все шесть
  оставшихся red-файлов лезут на алиасы прогона
  `www1.127.0.0.1`/`:18443`, которые движок рубит как mixed-content
  ещё до DNS (механизм `mixed-content-blocked`, владелец — `WPT-RUN-10`).
- `dedicated-worker-import-failure.html`: `new Worker('http://invalid:123$',
  {type:'module'})` обязан бросить `SyntaxError` (DOMException) — разбор URL
  в конструкторе не бросает вовсе, ни в классическом режиме, ни в
  модульном.
- `import.meta.url` внутри модульного воркера работает (общий
  `transform_import_meta` загрузчика), а вот `import.meta.resolve`
  и фрагмент в URL не проверялись.
- Модульный воркер по `blob:`/`data:` URL: тело исполняется как модуль,
  но относительные импорты из него резолвиться не могут — у такого
  воркера нет базы (то же ограничение, что у `importScripts`, BUG-778).

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
