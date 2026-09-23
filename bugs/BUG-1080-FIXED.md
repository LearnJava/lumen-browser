# BUG-1080 — в глобальной области dedicated/shared воркера нет `TextEncoder`/`TextDecoder`/`ReadableStream`/`TextDecoderStream`/`TextEncoderStream`: `ReferenceError: TextDecoder is not defined`

**Статус:** FIXED 2026-09-24 (P1, WORKER-1 срез 2)
**Тип:** пробел реализации — интерфейсы есть в оконной прелюдии и отсутствуют в воркерной области; какой из двух классов (точечный дефект или доработка вроде [GAP-WORKERSCOPE](../ROADMAP.md)) — решает триаж P3 по правилу [docs/probe-method.md §8](../docs/probe-method.md).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** воркерный рантайм — `crates/js/src/worker.rs` (воркерная глобальная область собирается из отдельных шимов: `WORKER_TIMERS_SHIM`, `WORKER_NET_SHIM`, `WORKER_ATOB_BTOA_SHIM`, …; шима кодировок/стримов среди них нет), `crates/js/src/shared_worker.rs`
**Владелец:** P3.

## Симптом

Прогон `encoding` без `legacy-mb-*` (132 id, `--update-expected`, 2026-09-22): в логе `TextDecoder is not defined` 15 179 раз, `TextDecoderStream is not defined` 144,
`TextEncoder is not defined` 112, `ReadableStream is not defined` 98, `TextEncoderStream is not defined` 8. Все — подтесты **воркерных** вариантов (51 id), в HTML-отчёте
каждый подтест `FAIL … X is not defined`:

- `api-basics.any.worker.html` 0/6, окно `api-basics.any.html` — 6/6 (тот же исходник);
- `textdecoder-labels.any.worker.html` 0/222, `encodeInto.any.worker.html` и `.sharedworker.html` 0/111, `api-invalid-label.any.worker.html` (4 query-варианта) 0/3421,
  `textdecoder-fatal-single-byte.any.worker.html` (8 query-вариантов) 0/8168, `textdecoder-mistakes.any.worker.html` 0/87;
- `streams/*.any.worker.html`/`.any.sharedworker.html` — 12 файлов × 2, все подтесты `FAIL`.

Окно тех же интерфейсов не лишено (`api-basics.any.html` 6/6, проба `--dump-layout`: `TextEncoderStream`, `TextDecoderStream`, `ReadableStream` — функции).

## Ожидание

`TextEncoder`/`TextDecoder` (Encoding Standard) и `TextEncoderStream`/`TextDecoderStream` — `[Exposed=*]`; `ReadableStream` (Streams Standard) — `[Exposed=*]`. Тесты `encoding/*.any.js`
объявляют `// META: global=window,worker`.

## Гипотеза (не проверена)

`worker.rs` строит воркерную область из набора `WORKER_*_SHIM`; `TextDecoder` в нём только у родителя (`new TextDecoder().decode(blob._bytes)` в `WORKER_SHIM`, стр. ~1347 — это
главный поток, разбирающий blob-URL). Значит шим кодировок/стримов в воркерный изолят просто не ставится. Проверка изнутри воркера (`typeof self.TextDecoder`) не делалась — проба
`--dump-layout` не ждёт `postMessage` от воркера.

## Связанное

- [BUG-1066](BUG-1066-FIXED.md) (`DOMException`), [BUG-1071](BUG-1071-OPEN.md) (`WebSocket`), [BUG-1076](BUG-1076-OPEN.md) (`Worker`), [BUG-1078](BUG-1078-OPEN.md) (`WebAssembly.*Streaming`) — тот же класс:
  интерфейс есть в окне и отсутствует в воркерной области.
- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.

## Не проверялось

- Полный набор отсутствующих в воркере интерфейсов (`WritableStream`, `TransformStream`, `CompressionStream`…) — по логу известны только пять перечисленных.
- Service-worker-варианты: в baseline они `ERROR` на https-origin ([BUG-1069](BUG-1069-FIXED.md)), до кода не доходят.

## Прогресс

- **2026-09-23, WORKER-1 срез 1:** `TextEncoder`/`TextDecoder` вырезаны из
  `web_api_shim_mid_b2.js` в `shim/text_encoding_shim.js` (дословный срез) и входят в
  `dom::worker_exposed_shim()`; нативы `_lumen_text_encoding_for_label`/`_lumen_text_decode`
  регистрирует `dom::install_worker_exposed_v8` — все три вида воркеров. Остаток бага:
  `ReadableStream`, `TextDecoderStream`/`TextEncoderStream`.
  WPT `encoding` (без `legacy-mb-*`, `run_report.py --update-expected`): ≈3 000 записей
  `expected: FAIL` у `.any.worker.html`/`.any.sharedworker.html` сняты, 7 `.ini` стали
  чистыми. Service-worker-варианты перешли ERROR→TIMEOUT — это дрейф baseline, на бинаре
  main они ведут себя так же (проверено `run_smoke.py`).
- **2026-09-24, WORKER-1 срез 2 — закрыт:** блок Streams (`ReadableStream`/`WritableStream`/
  `TransformStream`, стратегии, `TextDecoderStream`/`TextEncoderStream`, `CompressionStream`/
  `DecompressionStream`) дословно вырезан из хвоста `web_api_shim_mid_b.js` в
  `shim/streams_shim.js` и входит в `worker_exposed_shim()`; нативы `_lumen_cs_*` регистрирует
  `install_worker_exposed_v8`. Страничный шим побайтно прежний. Приёмка вскрыла дефект цикла
  задач воркера (таймер, заведённый из `.then()`, не попадал в расчёт ожидания — воркер засыпал
  навсегда), исправлен в `crates/js/src/worker.rs`. WPT `streams/`+`compression/`: ≈1 340
  воркерных подтестов FAIL→PASS, baseline перегенерирован; оконные изменения в
  `streams/transferable/*.html` — чужой дрейф (A/B с бинарём main одинаков), в baseline не взяты.
