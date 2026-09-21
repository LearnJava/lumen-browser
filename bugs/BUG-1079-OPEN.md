# BUG-1079 — оконные `WebAssembly.compileStreaming`/`instantiateStreaming` не проверяют аргументы и отдают не-спецификационные результаты

**Статус:** OPEN
**Тип:** несоответствие спецификации — стриминговые методы в оконном шиме (`crates/js/src/webassembly.rs:342-357`) переиспользуют `compile`/`instantiate` без проверок алгоритма streaming
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 48, `wasm`)
**Область:** js — `crates/js/src/webassembly.rs` (`compileStreaming`, `instantiateStreaming`, `Instance.exports`)
**Владелец:** P3.

## Симптом

Одиночный прогон `wasm/webapi` (2026-09-22, оконные варианты):

- `instantiateStreaming-bad-imports.any.html` — 9/106: `Non-object imports argument: null`/`true`/`""` … — `assert_unreached: Should have rejected: undefined`
  (промис резолвится, а не отклоняется `TypeError`);
- `invalid-args.any.html` — 4/44: `compileStreaming: undefined` и `… in a promise` — отклоняется `CompileError: Module requires a BufferSource`, тест ждёт `TypeError`;
- `instantiateStreaming.any.html` — 0/25: `assert_false: extensible exports expected false got true` — объект `exports` расширяем, по спецификации это frozen-объект с
  null-прототипом (у не-стримингового `instantiate()` не проверялось).

## Ожидание

- аргумент, не являющийся `Response`/промисом на `Response`, — отклонение `TypeError`; `Content-Type` не `application/wasm` — `TypeError`; не-`ok` статус — `TypeError`;
- `importObject`, не являющийся объектом или `undefined`, — отклонение `TypeError`;
- `WebAssembly.Instance.prototype.exports` — замороженный объект с `null`-прототипом.

## Попутно (отдельно не заводилось)

`WebAssembly.Global.prototype.type` отсутствует (24 сообщения `myglobal.type is not a function`, `jsapi/global/type.tentative.any.js`) — предложение type reflection, тест
помечен `tentative`.

## Связанное

- [BUG-1078](BUG-1078-OPEN.md) — в воркерах этих методов нет вовсе.
- `docs/tasks/p2-test-track.md#test-3-срез-48-2026-09-22`.
