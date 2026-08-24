# BUG-872 — у воркерной глобальной области нет ни одного интерфейсного объекта: `self instanceof DedicatedWorkerGlobalScope` — `ReferenceError`, `self.constructor` — `undefined`

**Статус:** OPEN
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
