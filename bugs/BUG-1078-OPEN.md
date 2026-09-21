# BUG-1078 — в воркерах нет `WebAssembly.compileStreaming`/`instantiateStreaming`: `TypeError: WebAssembly.instantiateStreaming is not a function`

**Статус:** OPEN
**Тип:** пробел реализации — стриминговые методы есть только в оконной прелюдии (`crates/js/src/webassembly.rs:342-375`, установка `crates/js/src/v8_runtime.rs:1034`); в воркерной области их нет
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 48, `wasm`)
**Область:** js — воркерный рантайм (`crates/js/src/worker.rs`, `shared_worker`), установка WebAssembly-шима
**Владелец:** P3.

## Симптом

Прогон `wasm` (612 id, `--update-expected`, 2026-09-22): в логе ~364 сообщения `WebAssembly.instantiateStreaming is not a function` (212) и
`WebAssembly[method] is not a function` (152). Одиночный прогон `run_report.py --all --root wasm/webapi --recursive --exclude-prefix /wasm/webapi/esm-integration`
(56 id) привязал их к воркерным вариантам:

- `instantiateStreaming-bad-imports.any.worker.html` — 0/106, каждый подтест `FAIL … WebAssembly.instantiateStreaming is not a function`;
- `invalid-args.any.worker.html` — 0/44, `FAIL … WebAssembly[method] is not a function`;
- тот же класс — воркерные варианты остальных `webapi/*.any.js` (`abort`, `body`, `contenttype`, `status`, `rejected-arg`, `wasm_stream_*` …) — не пересчитывалось пофайлово.

Проба страницы (`--dump-layout`, окно): `typeof WebAssembly.compileStreaming === 'function'`, `typeof WebAssembly.instantiateStreaming === 'function'`,
`instantiateStreaming(new Response(bytes, {headers: {'Content-Type': 'application/wasm'}}))` резолвится. То есть дефект именно воркерной области.

## Ожидание

Тесты `wasm/webapi/*.any.js` объявляют `// META: global=window,worker` и ждут `compileStreaming`/`instantiateStreaming` в воркерных областях
(WebAssembly Web API, §Streaming Module Compilation and Instantiation); точную декларацию `[Exposed]` в спецификации не сверял.

## Гипотеза (не проверена)

Шим ставится из `install_webassembly_bindings_v8` в оконном рантайме; в `worker.rs` ссылок на `WebAssembly` нет, значит воркерный изолят, вероятно, видит встроенный
`WebAssembly` V8 без стриминговых методов (у встроенного они появляются только при колбэке эмбеддера). Чем именно отличаются два объекта в воркере — не смотрелось.

## Связанное

- [BUG-1079](BUG-1079-OPEN.md) — оконная реализация тех же методов не соответствует спецификации.
- `docs/tasks/p2-test-track.md#test-3-срез-48-2026-09-22`.

## Не проверялось

- `typeof WebAssembly` и набор его свойств внутри воркера напрямую (нужна проба, доставляющая результат из воркера синхронно или через `postMessage` с ожиданием).
- Service-worker и shared-worker варианты — в baseline они `ERROR`/`TIMEOUT` по другим причинам, отдельно не разбирались.
