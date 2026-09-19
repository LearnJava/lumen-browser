# BUG-905 — runtime-ошибка shared-воркера рассылается клиентам: `SharedWorker.onerror` обязан срабатывать только на провал загрузки скрипта

**Статус:** FIXED 2026-09-19 (P3)
**Заведён:** 2026-08-24 (P1, при замере [BUG-813](BUG-813-FIXED.md))
**Область:** `crates/js/src/shared_worker.rs` (`SHARED_WORKER_GLOBAL_SHIM`,
хвост `_lumen_sw_report_exception` — безусловный `_lumen_sw_report_error`
для неотменённой ошибки; `run_shared_worker_thread_v8` — рассылка и
`pending_error` для верхнеуровневого отказа), `crates/js/src/v8_runtime.rs`
(`eval_and_report_via` не разделяет фазу компиляции и фазу исполнения)
**Владелец:** P1/P3

## Симптом

HTML LS [§10.2.6 «Runtime script errors»](https://html.spec.whatwg.org/multipage/workers.html#runtime-script-errors-2)
пробрасывает необработанную ошибку на объект-владелец **только** для
выделенного воркера. У shared-воркера ошибка заканчивается в его
собственной области видимости и дальше идёт в консоль — на
`SharedWorker` она не приходит. Единственное, что там даёт `error`, —
провал *загрузки* скрипта, и тот плоским `Event`, а не `ErrorEvent`.

Lumen рассылает любую runtime-ошибку всем подключённым клиентам, поэтому
страничный `onerror` вызывается там, где тест ждёт тишины:

```js
// workers/SharedWorker-script-error.html, сокращённо
const worker = new SharedWorker("support/SharedWorker-script-error.js");
worker.onerror = () => assert_unreached("FAIL: onerror invoked for a script error.");
worker.port.postMessage("unhandledError");   // ← обработчик порта зовёт generateError()
```

## Прямое измерение

`run_smoke.py`, dev-release, 2026-08-24 (после фикса BUG-813):

| тест | результат |
|---|---|
| `workers/SharedWorker-script-error.html` | **ERROR** — «onerror invoked for a script error», при этом оба сабтеста PASS: страница получила лишний `error` |
| `workers/SharedWorker-exception-propagation.html` | PASS |
| `workers/SharedWorker-exception.html` | PASS |
| `workers/shared-worker-runtime-error-is-not-parse-error.html` | 2/3 — обе runtime-строки PASS, падает третья (см. ниже) |

Два PASS существенны: дефект узкий. Рассылка бьёт только тогда, когда
клиент **уже подключён** — то есть для ошибки из обработчика порта или из
`onconnect`. Верхнеуровневый отказ приходит раньше первого `Connect` и
доезжает до клиента через `pending_error`, но обе пробы показывают, что
до страницы он в этом виде не добирается.

## Причина

Дефект **предшествует** [BUG-813](BUG-813-FIXED.md) и им не внесён:
рассылка заведена [BUG-591](BUG-591-FIXED.md) (2026-08-23) — тогда
безусловный `_lumen_sw_report_error(...)` в хвосте
`_lumen_sw_report_exception`, и `catch(e) { _lumen_sw_report_exception(e); }`
вокруг вызова обработчика порта уже существовали. BUG-813 добавил перед
этим хвостом «report the exception» в саму область воркера, ничего не
убрав.

## Почему не починено вместе с BUG-813

Убрать рассылку для runtime-ошибки — три строки, но правильный фикс должен
одновременно оставить событие для провала **загрузки**, а эти два случая
сейчас неразличимы: `V8JsRuntime::eval_and_report_via` зовёт репортёра и
из ветки компиляции, и из ветки исполнения, поэтому «репортёр отработал»
не отделяет синтаксическую ошибку классического скрипта от исключения его
тела. Третий сабтест
`shared-worker-runtime-error-is-not-parse-error.html` («Script parse error
dispatches plain Event at SharedWorker») требует ровно этого различия и
падает сейчас по второй причине: движок шлёт `ErrorEvent` там, где спека
требует плоский `Event`.

## Направление починки (не предписание)

1. Развести в `eval_and_report_via` фазу компиляции и фазу исполнения —
   репортёр должен зваться только из второй; тогда «репортёр отработал»
   становится честным признаком runtime-ошибки.
2. В `_lumen_sw_report_exception` неотменённую ошибку писать в консоль,
   а не в `_lumen_sw_report_error`.
3. Провал загрузки/разбора оставить за Rust-веткой
   `run_shared_worker_thread_v8` и отдавать плоским `Event`, а не
   `ErrorEvent`.

Проверять по четырём тестам из таблицы выше; порог — `SharedWorker-script-error.html`
перестаёт быть ERROR, а `shared-worker-runtime-error-is-not-parse-error.html`
становится 3/3, при этом [BUG-777](BUG-777-FIXED.md)-случай (провал импорта
в модульном shared-воркере доходит до клиента) не ломается.

## Исправлено (2026-09-19, P3)

Все три направления из плана выше реализованы как есть:

1. Новый `V8JsRuntime::eval_and_report_via_runtime_only` (`v8_runtime/eval.rs`)
   — копия `eval_and_report_via`, но зовёт JS-репортёр только из ветки
   исполнения (`compiled.run(tc)`), не из ветки компиляции. Для классического
   скрипта это ровно то различие, которое модульный путь уже имел через
   `ModuleFailure::{Load,Runtime}` (`eval_module_at_and_report_via`) —
   `run_shared_worker_thread_v8` теперь читает
   `typeof globalThis._lumen_worker_error_cancelled === 'boolean'` как признак
   «репортёр вызывался», одинаково для classic- и module-веток.
2. `_lumen_sw_report_exception` (`SHARED_WORKER_GLOBAL_SHIM`) больше не зовёт
   `_lumen_sw_report_error` для неотменённой ошибки — пишет в консоль воркера
   через `_lumen_sw_console_log`. Натив `_lumen_sw_report_error` и параметр
   `error_ports` у `install_shared_worker_globals_v8` удалены целиком: ничто
   в JS их больше не вызывает — HTML LS не даёт shared-воркеру одного
   владельца, на которого можно переслать runtime-исключение, оно
   останавливается на самой области видимости независимо от отмены.
3. Единственный путь, который всё ещё достигает клиента, — top-level
   parse/load-провал (тело скрипта не стартовало вовсе). Он рассылается через
   новый `crate::worker::error_info_json_plain` (поле `"plain":true` в
   JSON-конверте). Клиентский `SharedWorker.prototype._deliverError`
   (`shared_worker.rs`'s `SHARED_WORKER_SHIM`) при этом флаге строит
   `new Event('error', …)` вместо `new ErrorEvent('error', …)` — третий
   сабтест `shared-worker-runtime-error-is-not-parse-error.html` («Script
   parse error dispatches plain Event») закрыт тем же изменением.

**Тесты:** 3 регресс-теста в `crates/js/src/dom/tests/v8_webworker.rs`
инвертированы — `shared_worker_onconnect_exception_does_not_reach_client_onerror`,
`shared_worker_port_onmessage_exception_does_not_reach_any_client`,
`shared_worker_error_addeventlistener_does_not_fire_for_runtime_error` (раньше
проверяли, что рассылка происходит; BUG-905 и был в том, что это неверно).
2 новых юнит-теста в `shared_worker.rs`:
`v8_shared_worker_top_level_parse_failure_reaches_the_client` (parse failure
доходит с `"plain":true`), `v8_shared_worker_top_level_throw_runs_the_scopes_own_onerror`
расширен на оба исхода отмены — ни один больше не доходит до клиента.
`v8_shared_worker_top_level_failure_reaches_the_first_client` переведён с
runtime `throw` на настоящий синтаксис-провал, поскольку runtime-версия этого
сценария (тело успело стартовать) с фиксом закономерно перестала доходить до
клиента.

`cargo test -p lumen-js --features v8-backend` зелёный (единственный
обнаруженный провал в полном прогоне — `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty`,
не связан с этой правкой и проходит в изоляции — существующая межтестовая
флакиность, не регрессия). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` чист.
