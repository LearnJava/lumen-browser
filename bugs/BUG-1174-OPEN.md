# BUG-1174: `WorkerNavigator.permissions` отсутствует — Permissions API в воркерах не установлен

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs` и соседи — установка воркерного глобала; `crates/js/src/permissions.rs` ставится только в окно)
**Найден:** P3, при починке [BUG-650](BUG-650-FIXED.md), 2026-09-25

## Симптом

W3C Permissions §5 объявляет `[Exposed=(Window,Worker)] Permissions` и
`WorkerNavigator.permissions`. В Lumen `install_permissions_api_v8` зовётся
только из оконного `install_dom` (`v8_runtime.rs`), воркерный глобал его не
получает: в `DedicatedWorker` `navigator.permissions === undefined`.

WPT: `permissions-request/idlharness.any.worker.html` — 3 FAIL на
`request(object)`, `permissions/idlharness.any.worker.html` — весь блок
`Permissions`/`PermissionStatus`/`WorkerNavigator.permissions` в `.ini`
как ожидаемые FAIL.

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root permissions-request --recursive
```

## Что нужно

Поставить `PERMISSIONS_SHIM` в воркерный глобал (нужны воркерные `EventTarget`
и `Event`), решить, откуда воркер берёт `notifications` (у воркера нет
`Notification.requestPermission`), и повесить `permissions` на
`WorkerNavigator.prototype`.
