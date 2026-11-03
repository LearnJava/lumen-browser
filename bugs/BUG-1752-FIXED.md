# BUG-1752 — JS-поток не прокачивает очередь задач платформы V8: `Atomics.waitAsync` не резолвится, нативная асинхронная компиляция wasm висит

**Статус:** FIXED 2026-11-03 (P1)
**Заведён:** 2026-10-27 (P2, WPT-WASM-SCOUT)
**Область:** js (`crates/js/src/v8_runtime/thread.rs` — цикл JS-потока)

## Симптом

```js
const i32 = new Int32Array(new SharedArrayBuffer(4));
const r = Atomics.waitAsync(i32, 0, 0);
Atomics.notify(i32, 0);
r.value.then(v => console.log(v));               // никогда не печатает "ok"
Atomics.waitAsync(i32, 0, 0, 10).value.then(...); // "timed-out" тоже не приходит
```

Проверено пробой на `dev-release` от `main` `1c0d87c52` (testharness-файл через `run_smoke.py`, копия пробы — `.tmp/probe-pump.html` слота `p2-work`): оба промиса не резолвятся за 2 с, оба сабтеста FAIL.

## Причина

V8 доставляет часть работы через очередь задач платформы (foreground task runner изолята): уведомление async-ожидающего `Atomics.waitAsync`, его таймаут, колбэки очистки `FinalizationRegistry`, завершение асинхронной компиляции `WebAssembly.compile`/`instantiate`. Эту очередь разбирает только `v8::Platform::pump_message_loop`, а в рабочем коде он вызывался лишь в путях сборки мусора (`V8JsRuntime::run_gc_pass`, `runtime.rs:1825`; `dom_core.rs:1247`). Цикл JS-потока (`thread.rs`) выполнял `V8Command::Run` и снова блокировался на `recv()` — задачи платформы лежали до случайного GC-прохода.

Сейчас wasm-категорию это не задевало только потому, что `WebAssembly` был подменён собственным интерпретатором (`webassembly.rs`, синхронный путь). Переход на нативный `WebAssembly` V8 (`WASM-V8-NATIVE`) без этого фикса давал 311 TIMEOUT из 612.

## Что сделано

1. `v8_thread_main` (`crates/js/src/v8_runtime/thread.rs`) ждёт команду через `recv_timeout`, а не `recv()`: шаг 1 мс, пока на каждый опрос находится работа, и удвоение до 32 мс, когда изолят простаивает (команда от шелла и найденная задача сбрасывают шаг обратно). Простаивающая вкладка просыпается ~30 раз в секунду, а не крутится в цикле.
2. Новый `run_platform_tasks`: `Platform::pump_message_loop` до опустошения очереди, `perform_microtask_checkpoint` после каждой задачи (там же выполняется `.then`, подцепленный к разрешённому промису) и `drain_promise_rejections`, если что-то исполнилось. Зовётся на каждом пробуждении по таймауту **и** после каждой `V8Command::Run`, не дожидаясь шага: задача могла встать уже во время самой задачи (фоновая компиляция wasm).
3. Прокачка обёрнута в `StallHandle::job_started/job_finished` и `StallSampler`, как обычная задача: колбэк из задачи платформы — это исполнение скрипта, и сторож («страница не отвечает», `any_hung()`) обязан его видеть (`subsystems/js.md` §Invariants).
4. Тот же цикл обслуживает окно, выделенные и разделяемые воркеры и изолят сервис-воркера: `worker.rs:928`, `shared_worker.rs:886`, `sw_worker.rs:559` создают `V8JsRuntime`, то есть `v8_thread_main`.

## Как проверить

- `cargo test -p lumen-js --features v8-backend settles_while_the_thread_waits` — 2 зелёных: `v8_runtime/tests/bug1752_platform_queue.rs` паркует `Atomics.waitAsync` (таймаут 20 мс и `notify`) и **спит 200 мс, не отправляя ни одной команды**, только потом читает результат. До фикса промис оставался `pending`, потому что разрешить его мог лишь следующий `eval`.
- `WASM-V8-NATIVE-S1` (следующий шаг): `run_corpus.py --categories wasm --prefixes wasm/core` без TIMEOUT на `Reinitialize the default imports` — проверяется уже на нативном `WebAssembly`.
