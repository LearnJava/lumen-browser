# BUG-1085 — `new WebAssembly.Memory({shared:true, …}).buffer` — обычный `ArrayBuffer`, а не `SharedArrayBuffer`; `shared:true` без `maximum` не бросает

**Статус:** OPEN
**Тип:** пробел реализации — общая память WebAssembly не реализована (создаётся неразделяемая); `SharedArrayBuffer` как конструктор при этом есть.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** js — `crates/js/src/webassembly.rs` (шим `WebAssembly.Memory`), возможно `crates/js/src/v8_runtime.rs`
**Владелец:** P3.

## Симптом

Проба страницы (`--dump-layout`, 2026-09-22):

- `typeof SharedArrayBuffer` → `function`; `new SharedArrayBuffer(8)` → `[object SharedArrayBuffer]`.
- `new WebAssembly.Memory({shared:true, initial:1, maximum:2}).buffer` → `[object ArrayBuffer]`.
- `new WebAssembly.Memory({shared:true, initial:1})` (без `maximum`) → создаётся; JS API требует `TypeError`.

Тестовый хелпер [`tests/wpt/common/sab.js`](../tests/wpt/common/sab.js) получает `SharedArrayBuffer` именно так (см. whatwg/html#5380) и, увидев не тот конструктор, бросает
`Error("WebAssembly.Memory does not support shared:true")`. В `encoding` это 55 подтестов: `encodeInto.any.html` (54) и `textdecoder-copy.any.html` (1). Хелпер подключают 12 файлов
из 5 категорий: `encoding` (4), `html/webappapis/structured-clone`, `html/infrastructure/safe-passing-of-structured-data` (2), `workers/semantics/structured-clone` (2), `webidl/ecmascript-binding` (2).

## Ожидание

`WebAssembly.Memory` с `shared:true` отдаёт `buffer` типа `SharedArrayBuffer` (WebAssembly JS API, `Memory`; threads proposal); без `maximum` — `TypeError`.

## Связанное

- [BUG-846](BUG-846-FIXED.md) — прежний `SharedArrayBuffer`-дефект.
- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.

## Не проверялось

- Работает ли `Atomics.wait/notify` над такой памятью; передача через `postMessage` (структурное клонирование) — только что упирается в тот же `sab.js`.
