# BUG-1086 — в воркерах нет Trusted Types: `trustedTypes is not defined` в Dedicated/Shared Worker

**Статус:** FIXED 2026-09-24 (P1, WORKER-1 срез 9, закрытие)
**Тип:** пробел реализации — шим Trusted Types ставился только в оконный рантайм; воркерные глобалы его не получали.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 51, `trusted-types`)
**Область:** js — `crates/js/src/trusted_types.rs` (`TRUSTED_TYPES_SHIM`), `crates/js/src/v8_runtime.rs:687` (страничный вызов, не тронут), `crates/js/src/worker.rs::install_worker_scope_globals_v8` (общая точка для всех трёх видов воркера)
**Владелец:** P1.

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

## Исправление (2026-09-24, P1, WORKER-1 срез 9)

`TRUSTED_TYPES_SHIM` — самодостаточная IIFE: собственный `SECRET`/`VALUES`-closure,
единственное касание page-only состояния — `if (typeof window !== 'undefined') { window.… = … }`,
защищённое `typeof`-гвардом. Она уже вызывалась только из страничного
`v8_runtime.rs::install_dom`; добавлен один `rt.eval(crate::trusted_types::TRUSTED_TYPES_SHIM)`
в конец общей `crate::worker::install_worker_scope_globals_v8` (`worker.rs`) — той же точки,
через которую срезы 1-8 WORKER-1 уже подключали Streams/WebAssembly-streaming/RAF ко всем
трём видам воркера разом (dedicated/shared/service — все три зовут эту функцию).

Новый тест `v8_worker_globals_have_trusted_types` (`worker.rs`): проверяет `typeof
self.trustedTypes === 'object'` и полный цикл `createPolicy`→`createHTML`→`isHTML`/`toString()`
внутри dedicated worker scope.

Гейт: `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` чист;
`cargo test -p lumen-js --features v8-backend --lib` — 4231/4231 зелёные (предсуществующий
флак BUG-1110 не воспроизвёлся в этом прогоне); `cargo clippy --workspace --all-targets --
-D warnings` чист. Живой WPT-прогон (`trusted-types/*.any.worker.html`) не выполнен — вне
объёма этого закрытия.

Закрывает WORKER-1 целиком (ROADMAP.md): все восемь перечисленных там BUGS (1080, 1071,
1076, 1078, 1081, 959, 766, 1086) теперь FIXED.
