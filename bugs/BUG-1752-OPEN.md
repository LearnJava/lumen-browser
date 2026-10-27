# BUG-1752 — JS-поток не прокачивает очередь задач платформы V8: `Atomics.waitAsync` не резолвится, нативная асинхронная компиляция wasm висит

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-WASM-SCOUT)
**Область:** js (`crates/js/src/v8_runtime/thread.rs` — цикл `while let Ok(cmd) = cmd_rx.recv()`)

## Симптом

```js
const i32 = new Int32Array(new SharedArrayBuffer(4));
const r = Atomics.waitAsync(i32, 0, 0);
Atomics.notify(i32, 0);
r.value.then(v => console.log(v));               // никогда не печатает "ok"
Atomics.waitAsync(i32, 0, 0, 10).value.then(...); // "timed-out" тоже не приходит
```

Проверено пробой на `dev-release` от `main` `1c0d87c52` (testharness-файл через `run_smoke.py`, копия пробы — `.tmp/probe-pump.html` слота `p2-work`): оба промиса не резолвятся за 2 с, оба сабтеста FAIL. С экспериментальной прокачкой (см. ниже) — оба PASS.

## Причина

V8 доставляет часть работы через очередь задач платформы (foreground task runner изолята): уведомление async-ожидающего `Atomics.waitAsync`, его таймаут, колбэки очистки `FinalizationRegistry`, завершение асинхронной компиляции `WebAssembly.compile`/`instantiate`. Эту очередь разбирает только `v8::Platform::pump_message_loop`, а в рабочем коде он вызывается лишь в путях сборки мусора (`V8JsRuntime::run_gc_pass`, `runtime.rs:1825`; `dom_core.rs:1247`) и в тестах. Цикл JS-потока (`thread.rs`) выполняет `V8Command::Run` и снова блокируется на `recv()` — задачи платформы лежат до случайного GC-прохода. Пробел уже описан в комментариях (`runtime.rs:1803–1814`, `tc39_proposals.rs:1135–1147`), но как дефект не заведён.

Сейчас wasm-категорию это не задевает только потому, что `WebAssembly` подменён собственным интерпретатором (`crates/js/src/webassembly.rs`, синхронный путь). Переход на нативный `WebAssembly` V8 (`WASM-V8-NATIVE` в `ROADMAP.md`) без этого фикса даёт 311 TIMEOUT из 612: цепочка `WebAssembly.compile(...)` в общем harness `wasm/core/js/harness/async_index.js` не продвигается дальше первого промиса.

## Что делать

1. После каждой `V8Command::Run` в `thread.rs` — `while Platform::pump_message_loop(&platform, &isolate, false) {}` и `perform_microtask_checkpoint()` (так сделан эксперимент: diff — `.tmp/wasm-native-experiment.diff` слота `p2-work`).
2. Этого мало: задача платформы может появиться, когда JS-поток спит в `recv()` (фоновая компиляция завершилась, истёк таймаут `waitAsync`). Нужен способ разбудить поток — `recv_timeout` с коротким шагом, пока в очереди могут быть задачи, или собственная `v8::Platform`, которая при постановке задачи шлёт в канал команду-пробуждение. В эксперименте с прокачкой «только после команды» 10 огромных spec-файлов (`wasm/core/f32.wast.js.html`, `simd_f32x4_*` …) упираются в 10-секундный лимит (5151/5152 сабтестов — пропускная способность цепочки ограничена частотой команд от шелла).
3. То же для потоков воркеров (`worker.rs` строит свой `V8JsRuntime` — проверить, что идёт через тот же цикл).

## Как проверить

- Проба выше: оба сабтеста PASS.
- Модульный тест в `crates/js`: `Atomics.waitAsync` + `notify` резолвится без ручного `pump_message_loop` в тесте.
- После `WASM-V8-NATIVE`: `run_corpus.py --prefixes wasm/core` — нет TIMEOUT на `Reinitialize the default imports`.
