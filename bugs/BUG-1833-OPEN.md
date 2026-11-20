# BUG-1833-OPEN: shared-worker реалм — shim-функции печатают исходник, top-level failure не доходит до клиента

- **Статус:** OPEN
- **Область:** js (`crates/js/src/shared_worker.rs` — `install_shared_worker_globals_v8`, run-путь shared worker; реестр `shim/native_registry_shim.js`)
- **Приоритет:** P3 (фингерпринт-дивергенция в SharedWorker-реалме; детерминированные падения `cargo test -p lumen-js --lib --features v8-backend`)

## Симптом

Два теста `shared_worker::tests_v8` стабильно красные на main `3a8243af5` (WS-A влит,
js-дерево с тех пор не менялось — дифф WS-B js не трогает):

1. `shared_worker_shim_functions_print_as_native` — `TextEncoder.prototype.encode`,
   `performance.now`, `fetch` в shared-worker реалме печатают исходник шима вместо
   `[native code]` (паника в `shared_worker.rs:1311`, дамп трёх функций в assert).
   То есть фасад `Function.prototype.toString` либо не установлен, либо эти функции
   установлены после `_lumen_native_sweep()` внутри
   `install_shared_worker_globals_v8` / run-пути.
2. `v8_shared_worker_top_level_failure_reaches_the_first_client` — клиент не получает
   событие ошибки top-level скрипта (`client never heard the failure: []`,
   `shared_worker.rs:2033`).

Примечание: живая проба `scripts/probe_native_api.py` (WS-A) показывала 0 утечек в
shared worker — вероятно, проба покрывает другой набор/порядок установки, чем
прямой вызов `install_shared_worker_globals_v8` в тесте. Расхождение пробы и
юнит-теста — часть диагноза.

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

## Критерий закрытия

Оба теста зелёные; `probe_native_api.py` и юнит-уровень согласованы (проба ловит
то же, что тест).

Заведён P3 при гейте FINGERPRINT-MANIFEST (scoped-test), 2026-11-20.
