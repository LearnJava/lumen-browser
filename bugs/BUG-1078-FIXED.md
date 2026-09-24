# BUG-1078 — в воркерах нет `WebAssembly.compileStreaming`/`instantiateStreaming`: `TypeError: WebAssembly.instantiateStreaming is not a function`

**Статус:** FIXED 2026-09-24 (P1, WORKER-1 срез 6)
**Тип:** пробел реализации — стриминговые методы есть только в оконной прелюдии (`crates/js/src/webassembly.rs:342-375`, установка `crates/js/src/v8_runtime.rs:1034`); в воркерной области их не было
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 48, `wasm`)
**Область:** js — воркерный рантайм (`crates/js/src/worker.rs`, `shared_worker`), установка WebAssembly-шима
**Владелец:** P1.

## Симптом

Прогон `wasm` (612 id, `--update-expected`, 2026-09-22): в логе ~364 сообщения `WebAssembly.instantiateStreaming is not a function` (212) и
`WebAssembly[method] is not a function` (152). Одиночный прогон `run_report.py --all --root wasm/webapi --recursive --exclude-prefix /wasm/webapi/esm-integration`
(56 id) привязал их к воркерным вариантам:

- `instantiateStreaming-bad-imports.any.worker.html` — 0/106, каждый подтест `FAIL … WebAssembly.instantiateStreaming is not a function`;
- `invalid-args.any.worker.html` — 0/44, `FAIL … WebAssembly[method] is not a function`;
- тот же класс — воркерные варианты остальных `webapi/*.any.js` (`abort`, `body`, `contenttype`, `status`, `rejected-arg`, `wasm_stream_*` …) — не пересчитывалось пофайлово.

Проба страницы (`--dump-layout`, окно): `typeof WebAssembly.compileStreaming === 'function'`, `typeof WebAssembly.instantiateStreaming === 'function'`,
`instantiateStreaming(new Response(bytes, {headers: {'Content-Type': 'application/wasm'}}))` резолвится. То есть дефект именно воркерной области.

## Причина

`install_webassembly_bindings_v8` (стриминговые методы `WebAssembly`) вызывалась только из оконного рантайма (`v8_runtime.rs::install_dom`).
`worker.rs` её не звал — воркерный изолят видел встроенный `WebAssembly` V8 без стриминговых методов (у встроенного они появляются только при
колбэке эмбеддера).

## Исправление

`install_webassembly_bindings_v8(rt)?` вызывается из `install_worker_scope_globals_v8` — общей точки установки, через которую проходят
dedicated/shared/service воркеры разом (тот же паттерн, что и остальная `[Exposed=Worker]`-поверхность WORKER-1, срезы 1–5). Шим
`compileStreaming`/`instantiateStreaming` читает `resp.arrayBuffer()` только в момент вызова, поэтому порядок установки относительно
`worker_net`'s `Response` (ставится позже, `install_worker_globals_v8`/`sw_worker`) не важен.

Новый тест `worker::tests_v8::v8_worker_globals_have_wasm_streaming` проверяет `typeof` всех пяти методов `WebAssembly`
(`compileStreaming`/`instantiateStreaming`/`compile`/`instantiate`/`validate`) внутри dedicated-worker scope.

Гейт: `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` чист; `cargo test -p lumen-js --features v8-backend --lib`
4222/4223 (единственный красный — предсуществующий флак `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty` при параллельном
запуске, [BUG-1110](BUG-1110-OPEN.md), зелёный при `--test-threads=1`, не связан); `cargo clippy --workspace --all-targets -- -D warnings` чист;
`scripts/scoped-test.sh` — тот же единственный флак, `lumen-network` в этот раз дошёл до конца без BUG-805-зависания.

Живой WPT-прогон не выполнен — статус построен на юнит-тесте, дословно проверяющем `typeof` внутри воркерного изолята; `run_report.py`
по `wasm/webapi/*.any.worker.html` — следующий шаг для количественного подтверждения (не в объёме этого среза).

## Связанное

- [BUG-1079](BUG-1079-OPEN.md) — оконная реализация тех же методов не соответствует спецификации (отдельный, не тронут этим срезом).
- `ROADMAP.md` — WORKER-1 срез 6.
