# BUG-1174: `WorkerNavigator.permissions` отсутствует — Permissions API в воркерах не установлен

**Статус:** FIXED 2026-11-15 (P3)
**Компонент:** js (`crates/js/src/worker.rs`, `crates/js/src/permissions.rs`)
**Найден:** P3, при починке [BUG-650](BUG-650-FIXED.md), 2026-09-25

## Симптом

W3C Permissions §5 объявляет `[Exposed=(Window,Worker)] Permissions` и
`WorkerNavigator.permissions`. В Lumen `install_permissions_api_v8` звался
только из оконного `install_dom` (`v8_runtime.rs`), воркерный глобал его не
получал: в `DedicatedWorker` `navigator.permissions === undefined`.

WPT: `permissions-request/idlharness.any.worker.html` — 3 FAIL на
`request(object)`, `permissions-revoke/idlharness.any.worker.html` — 3 FAIL на
`revoke(object)` (остаток [BUG-652](BUG-652-FIXED.md)),
`permissions/idlharness.any.worker.html` — весь блок
`Permissions`/`PermissionStatus`/`WorkerNavigator.permissions` в `.ini` как
ожидаемые FAIL.

## Причина

Единственная точка установки — `v8_runtime::install_dom` (страничный
`install_dom`). `install_worker_scope_globals_v8` (`worker.rs`), общий для
dedicated/shared/service-воркеров, ставил `EventTarget`/`Event` и
`WorkerNavigator`, но Permissions API не звал вовсе.

## Исправление

- `install_worker_scope_globals_v8` зовёт `permissions::install_permissions_api_v8`
  сразу после `install_worker_exposed_v8` (нужны его `EventTarget`/`Event` и
  `navigator`) — то есть во всех трёх видах воркеров, из одной точки.
- Шим ставит `permissions` в WebIDL-форме: в воркере нет страничного прохода
  `finalize_navigator_interface_v8`, поэтому readonly-аксессор с brand-check
  кладётся прямо на `WorkerNavigator.prototype` (на странице — прежний own
  property, который финализация переносит на `Navigator.prototype`).
- Заодно догнана форма интерфейсов, на которой `idlharness` падал и в странице:
  `prototype` интерфейсного объекта теперь non-writable (WebIDL §3.7.4),
  `[[Prototype]]` у `PermissionStatus` — `EventTarget` (§3.7.2), аксессоры
  атрибутов носят имена `get <attribute>`/`set <attribute>` (§3.7.7).
- `notifications` у воркера брать неоткуда: `Notification` не установлен, и
  `LIVE`-резолвер по `typeof` отвечает `denied` — `request()` до `ASK` не
  доходит. Это поведение default-closed, а не заглушка `granted`.

## Тесты

`worker::tests_v8::v8_worker_scope_installs_the_permissions_api` —
`navigator.permissions instanceof Permissions`, интерфейсы на месте,
аксессор на `WorkerNavigator.prototype` (не own property синглтона, имя
`get permissions`), `query({name:'camera'})` → `denied`, `notifications` →
`denied`.
`permissions::tests::interfaces_have_the_shape_idlharness_reads` — та самая
форма; страничная половина — 36 тестов `permissions::tests` без изменений.

## WPT

`run_report.py --all --root permissions-request|permissions-revoke --recursive`:
**2/2 harness OK, 14/14 сабтестов** в обеих, `.ini` удалены как чистые,
`--check` — 0 регрессий.
`run_report.py --all --root permissions --recursive`: **16/18 harness OK,
179/188 сабтестов**; `idlharness.any.html` и `idlharness.any.worker.html` —
**OK, 48/48** каждый, `idlharness.any.js.ini` удалён. Остальные FAIL/TIMEOUT
категории — про состояние разрешений под iframe/`test_driver`, не про этот
дефект; их baseline обновился попутно (старый писал `expected: ERROR` всем
`.https.`-файлам на уже закрытом TLS-гэпе), `--check` — 0 регрессий.

