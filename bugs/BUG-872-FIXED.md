# BUG-872 — у воркерной глобальной области нет ни одного интерфейсного объекта: `self instanceof DedicatedWorkerGlobalScope` — `ReferenceError`, `self.constructor` — `undefined`

**Статус:** FIXED 2026-09-20 (P6, GAP-WORKERSCOPE срез 1)
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-WORKERSCOPE` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `worker-global-interfaces`)
**Область:** `crates/js/src/worker.rs:338`+ (`worker_global_shim`) и `crates/js/src/shared_worker.rs:121`+ (`SHARED_WORKER_GLOBAL_SHIM`) — оба шима вешают на `globalThis` только функции (`postMessage`, `importScripts`, `close`, аксессоры `onmessage`), но не создают ни интерфейсных объектов (`WorkerGlobalScope`, `DedicatedWorkerGlobalScope`, `SharedWorkerGlobalScope`), ни прототипной цепочки для самой области
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Внутри воркера отсутствуют все восемь проверенных имён, а сама область —
голый объект без конструктора:

```
WorkerGlobalScope=false DedicatedWorkerGlobalScope=false
SharedWorkerGlobalScope=false MessageEvent=false MessageChannel=false
MessagePort=false WorkerNavigator=false ErrorEvent=false
ctor=            instanceof-throws=ReferenceError
```

(та же картина в shared-области: `WorkerGlobalScope=false`,
`SharedWorkerGlobalScope=false`, `MessageEvent=false`).

## Почему это не косметика

Стандартная идиома WPT — и вообще любого кода, который должен работать и в
dedicated-, и в shared-воркере — это ветка по типу области:

```js
if ('DedicatedWorkerGlobalScope' in self && self instanceof DedicatedWorkerGlobalScope) {
  postMessage('LOADED');
} else if ('SharedWorkerGlobalScope' in self && self instanceof SharedWorkerGlobalScope) {
  self.onconnect = e => { e.ports[0].postMessage('LOADED'); };
}
```

Это буквальный текст `workers/modules/resources/post-message-on-load-worker.js`.
Здесь обе ветки ложны, воркер молча не делает **ничего** — не бросает, не
пишет в лог, — и страница ждёт `LOADED` до таймаута.

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant worker-global-interfaces`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c`, `--seconds 7`) —
воркер сам перечисляет, что у него есть; вывод выше.

Что при этом **работает** и с чем не путать (проверено соседним вариантом
`worker-unsolicited-post`): `self.postMessage("LOADED")` без всякого повода,
до любого входящего сообщения, доходит до страницы и у dedicated-, и у
shared-воркера. То есть подозрение на BUG-815 (таймеры воркера флашатся
только при доставке сообщения) здесь ни при чём — молчит именно ветка
`instanceof`.

Диагностически показательная деталь отчёта: у
`workers/modules/dedicated-worker-options-type.html` подтест **default**-типа
стоит TIMEOUT, а `classic`/`module` — NOTRUN (`promise_test` идут
последовательно), тогда как два `test()` про невалидный `type` доехали до
честного FAIL. То есть файл ломается не на модульности (это
[BUG-777](BUG-777-FIXED.md)), а на самом первом, «обычном» воркере — на этом
баге.

## Масштаб

Прямо на нём стоят `workers/modules/dedicated-worker-options-type.html` и
`shared-worker-options-type.html` (по 3 зависших подтеста), и он же —
предусловие для `MessageEvent`-фактов [BUG-867](BUG-867-OPEN.md) и
`MessageChannel` из [BUG-868](BUG-868-OPEN.md): те два бага про поведение,
этот — про сами интерфейсные объекты, чинить их можно одним фрагментом
шима.

## Направление починки (не предписание)

Общие фрагменты у воркерной области уже есть — `EVENT_TARGET_SHIM` и
`PERFORMANCE_SHIM` попадают туда через
`worker::install_worker_scope_globals_v8` (BUG-401). Тем же способом
завести `WorkerGlobalScope` и двух его наследников, назначить глобалу
прототип соответствующего класса (`Object.setPrototypeOf(globalThis, …)`
после сборки методов) и вынести туда же `MessageEvent`/`ErrorEvent`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant worker-global-interfaces` — ожидается `DedicatedWorkerGlobalScope=true`,
   `instanceof=true`, непустой `ctor`.
2. WPT: `run_report.py --all --root workers/modules --recursive`.

## Исправлено

`WorkerGlobalScope`/`DedicatedWorkerGlobalScope`/`WorkerNavigator`/`ErrorEvent` уже отвечали
правильно на момент триажа этого среза (пришли по дороге с [BUG-777](BUG-777-FIXED.md)/
[BUG-776](BUG-776-FIXED.md)/[BUG-813](BUG-813-FIXED.md) — проверено регрессионным тестом
`v8_worker_scope_exposes_its_own_global_scope_interface`, `worker.rs`). Оставшаяся тройка —
`MessageEvent`/`MessageChannel`/`MessagePort` — получила два фрагмента шима, эвалируемых из
`worker::install_worker_scope_globals_v8` (общей точки для dedicated/shared/service-worker
областей, BUG-401):

- **`WORKER_MESSAGE_EVENT_SHIM`** (`worker.rs`) — автономный конструктор `MessageEvent`,
  тем же способом и тем же условным `typeof globalThis.X !== 'function'`, каким уже был решён
  `ErrorEvent` (`WORKER_ERROR_EVENT_SHIM`, BUG-813): своя копия вместо среза страничного
  `Event`-наследования, поскольку та цепочка — `WEB_API_SHIM_MID`, страничный код, тянущий
  `document`/`window`. Сигнатура конструктора — `new MessageEvent(data, init)` (данные первым
  аргументом, не WebIDL-тип), тот же нестандартный, но уже устоявшийся в этой кодовой базе
  контракт, что у страничного `MessageEvent` (`web_api_shim_mid_b2.js`) и `BroadcastChannel`
  (`broadcast_channel.rs`).
- **`MESSAGE_CHANNEL_SHIM`** — тот же самый файл шима, что страница и service-worker уже
  эвалировали (`crate::dom::MESSAGE_CHANNEL_SHIM`), теперь эвалируется и для dedicated/shared
  worker через общую точку — с тем же самым `MessageChannel`/`MessagePort`, без второй копии.

Пять точек, где воркерная/страничная область строила событие сообщения как голый литерал
(`{ type: 'message', data: ..., target: ... }`), переведены на реальный `new MessageEvent(...)`
— `e instanceof MessageEvent` теперь истинно и для доставленного сообщения, а не только для
конструктора на `self`: `worker.rs`'s `_lumen_worker_dispatch_message` (воркер получает от
страницы) и `Worker.prototype._deliver` (страница получает от воркера), `shared_worker.rs`'s
`_makePort._deliver` (воркер получает от клиентского порта) и `_makeClientPort._deliver`
(клиент получает от воркерного порта). Каждое место оборачивает конструктор в
`try{...}catch(e){...}` с откатом на прежний литерал — тот же защитный паттерн, что уже был в
`broadcast_channel.rs::_deliver` (комментарий там же объясняет, зачем: контекст, где
`MessageEvent` ещё не установлен глобально, не должен ронять доставку).

Не в этом срезе: `MessagePort.postMessage()` внутри самой воркерной/service-worker области
по-прежнему бросает `structuredClone is not defined` — тот же `MESSAGE_CHANNEL_SHIM` зависит от
`structuredClone`, а этот глобал определён только в страничном `WEB_API_SHIM_TAIL_B`
(`structuredClone`, `web_api_shim_tail_b.js:562`). Это не регрессия текущего среза: то же самое
было верно и для service-worker области до него — воркерный `MessageChannel` был
`ReferenceError` и раньше просто не доходил до этой стадии. И, главное, **перенос**
`MessagePort` через саму границу «страница ↔ воркер» ([BUG-868](BUG-868-OPEN.md)) этим срезом
не тронут — `new MessageChannel()` внутри воркера остаётся локальным этой области объектом,
`transfer`-список у `postMessage` воркера по-прежнему теряется.

`cargo test -p lumen-js --lib --features v8-backend` 3961/3961, `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` чист. Только JS-шим воркерной области,
пиксели не затронуты.
