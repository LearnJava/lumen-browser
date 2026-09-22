# BUG-1086 — в воркерах нет Trusted Types: `trustedTypes is not defined` в Dedicated/Shared Worker

**Статус:** OPEN
**Тип:** пробел реализации — шим Trusted Types ставится только в оконный рантайм; воркерные глобалы его не получают.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 51, `trusted-types`)
**Область:** js — `crates/js/src/trusted_types.rs` (`TRUSTED_TYPES_SHIM`), `crates/js/src/v8_runtime.rs:687` (единственное место, где шим исполняется), `crates/js/src/worker.rs` (`rt.eval(...)` на строках ~396–439 — набор воркерных шимов), `shared_worker.rs`, `sw_worker.rs`
**Владелец:** P3.

## Симптом

Прогон `trusted-types` (`run_report.py --log-raw`, 2026-09-22): каждый воркерный вариант, чей скрипт обращается к `trustedTypes`, пишет в лог

```
[worker-0] v8 script error: Runtime("trustedTypes is not defined")
[shared-worker] [ERR]  trustedTypes is not defined
```

и страница-родитель ждёт `done` от воркера до harness-`TIMEOUT`. Масштаб в категории: из 28 не-service-worker воркерных id **23 `TIMEOUT`**, 3 `ERROR`, 2 `OK`
(`DedicatedWorker-*`, `SharedWorker-*`, `block-string-assignment-to-*-Worker-setTimeout-setInterval`, `trusted-types-reporting-for-*Worker-*`); ещё два
`should-*-csp-00N-worker.html` тоже `TIMEOUT`. Ни у одного воркерного `TIMEOUT` нет проходящих подтестов (почти все `0/0`): до первой проверки дело не доходит.

Причина найдена по коду: `TRUSTED_TYPES_SHIM` исполняется только в `v8_runtime.rs:687` (оконный рантайм), а `worker.rs` собирает свой набор шимов
(`worker_exposed_shim`, `WORKER_ERROR_EVENT_SHIM`, `WORKER_MESSAGE_EVENT_SHIM`, …) без него.

## Ожидание

`self.trustedTypes` (`TrustedTypePolicyFactory`), `TrustedHTML`/`TrustedScript`/`TrustedScriptURL`/`TrustedTypePolicy` доступны в `DedicatedWorkerGlobalScope` и `SharedWorkerGlobalScope`
(Trusted Types §2.2 — `[Exposed=(Window,Worker)]`); `ServiceWorkerGlobalScope` — то же.

## Связанное

- [BUG-946](BUG-946-OPEN.md) — ни один sink не читает политику (оконная сторона); после появления `trustedTypes` в воркере воркерные тесты упрутся в него.
- [BUG-1087](BUG-1087-OPEN.md) — форма интерфейсов Trusted Types (WebIDL), отдельный дефект.
- [BUG-1069](BUG-1069-FIXED.md) — `*.https.html` воркерные варианты (15 service-worker id) упираются в https-origin и от этого бага не зависят.
- `docs/tasks/p2-test-track.md#test-3-срез-51-2026-09-22`.

## Не проверялось

- Что будет с `new Worker(TrustedScriptURL)` после появления шима (конструктор воркера принимает `TrustedScriptURL`; enforcement — часть BUG-946/BUG-811).
- Часть из 23 `TIMEOUT` может иметь и вторую причину (например, CSP-заголовки, BUG-811): проверялись только `DedicatedWorker-*` и `SharedWorker-*` — по сырому логу.
