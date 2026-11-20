# BUG-1833-FIXED: shared-worker реалм — shim-функции печатают исходник вместо `[native code]` (тест не повторяет run-путь)

- **Статус:** FIXED 2026-11-20 (дефект теста, не рантайма)
- **Область:** js (`crates/js/src/shared_worker.rs` — `install_shared_worker_globals_v8`, run-путь shared worker; реестр `shim/native_registry_shim.js`)
- **Приоритет:** P3 (фингерпринт-дивергенция в SharedWorker-реалме; детерминированное падение `cargo test -p lumen-js --lib --features v8-backend`)

## Симптом

`shared_worker::tests_v8::shared_worker_shim_functions_print_as_native` стабильно
красный на main `c6d618cf9`: `TextEncoder.prototype.encode`, `performance.now`,
`fetch` в shared-worker реалме печатают исходник шима вместо `[native code]`
(паника в `shared_worker.rs:1311`).

**Диагноз (2026-11-20):** тест вызывает `install_shared_worker_globals_v8`
напрямую, а sweep `_lumen_native_sweep()` по дизайну живёт не внутри install,
а в run-пути (`shared_worker.rs:927`) — после бинда WebSocket, последним шагом
перед worker-скриптом (комментарий `shared_worker.rs:1201-1203`). Тест sweep не
выполняет → функции, поставленные install'ом, фасадом не покрыты. Похоже на
дефект теста (не повторяет run-путь), а не рантайма: вживую sweep отрабатывает.

Второй симптом исходной записи — `v8_shared_worker_top_level_failure_reaches_the_first_client` —
на `c6d618cf9` **зелёный** (на `3a8243af5` падал детерминированно; вероятно,
закрыт upstream-фиксом `dc3df124b` или флаки — зафиксировать при повторном
падении).

Примечание: живая проба `scripts/probe_native_api.py` (WS-A) показывала 0 утечек
в shared worker — согласуется с диагнозом «тест не повторяет run-путь».

## Воспроизведение

```
cargo test -p lumen-js --lib --features v8-backend -- \
  shared_worker_shim_functions_print_as_native \
  v8_shared_worker_top_level_failure_reaches_the_first_client
```

Оба FAILED за ~1.4 с, детерминированно (повторный прогон — тот же результат).

## Гипотеза

В WS-A sweep для shared worker поставлен «последним шагом run-пути», но часть
глобалов (`TextEncoder`, `performance`, `fetch`) устанавливается внутри
`install_shared_worker_globals_v8` уже после точки sweep либо регистрируется
внутренним контейнером, который тестовый путь обходит. Второе падение может быть
следствием того же reorder run-пути (failure-ивент шлётся до подключения клиента).

## Разбор и фикс

Подтверждено на `c6d618cf9`: `v8_shared_worker_top_level_failure_reaches_the_first_client`
зелёный (закрыт upstream `dc3df124b` или флаки). `shared_worker_shim_functions_print_as_native`
падал, потому что вызывал `install_shared_worker_globals_v8` без
`_lumen_native_sweep()` — sweep живёт в run-пути (`shared_worker.rs:927`), после
бинда WebSocket и до worker-скрипта; `install` его намеренно не делает
(комментарий `shared_worker.rs:1201-1203`). Фикс — тест повторяет run-путь:
eval `crate::dom::NATIVE_REGISTRY_SWEEP` после install. Тест зелёный;
`probe_native_api.py` и юнит-уровень согласованы (рантайм утечек не имел).

Заведён P3 при гейте FINGERPRINT-MANIFEST (scoped-test), 2026-11-20; закрыт тем же днём.
