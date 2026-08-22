# BUG-846 — Compression Streams отдают результат только после `writer.close()`: чтение до закрытия записывающей стороны не разрешается никогда

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `compression-stream-read-before-close`)
**Область:** `crates/js/src/dom.rs:8215` (`CompressionStream` — `transform` только копит, вся работа в `flush`), `crates/js/src/dom.rs:8232` (`DecompressionStream` — то же), `crates/js/src/dom.rs:8197`–`8198` (комментарий шима, прямо описывающий модель «buffer-then-flush»)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
const ds = new DecompressionStream('deflate');
const reader = ds.readable.getReader();
ds.writable.getWriter().write(chunk);   // валидный deflate-чанк
await reader.read();                    // не разрешится никогда
```

Стоит добавить `writer.close()` — и та же цепочка отрабатывает мгновенно и
правильно.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 11 тиков; байты
сжатого «expected output» сгенерированы `zlib`, чтобы вход был заведомо
валидным):

| вариант | ожидалось | получено |
|---|---|---|
| `decompression-basic` (чтение без `close`) | `ds-read done=false text=expected output` | `cs-present ds=function cs=function`, `ds-after alive` — и всё |
| `decompression-formats` (gzip + deflate-raw, без `close`) | два `ds-…` | ни одного |
| `decompression-after-close` (контроль) | `ds-closed-read text=expected output` | ровно это |
| `compression-roundtrip` (`write` + `close` + чтение ридером) | `cs-chunk bytes>0` | `cs-chunk done=false bytes=35` |

То есть сам нативный кодек в порядке; ломается модель доставки.

## Причина (локализована чтением кода)

Шим прямо документирует свою модель:

```js
// dom.rs:8197
// Buffer-then-flush model: accumulates all input chunks, compresses atomically at
// flush (TransformStream.writable.close()). Emits a single Uint8Array output chunk.
```

`transform` (`dom.rs:8220`, `:8237`) только складывает чанк в массив, а
`c.enqueue` вызывается исключительно во `flush`. Спека
(<https://compression.spec.whatwg.org/>, §4 «transform algorithm») требует
отдавать всё, что кодек успел выдать, **на каждый чанк**: и deflate, и gzip
инкрементальны.

## Масштаб

Маркер `compression-stream-read-before-close` в `tests/wpt/timeout_audit.py` —
**3 id** остатка снимка WPT-RUN-5: `decompression-correct-input`,
`decompression-extra-input`, `decompression-uint8array-output`. Все три пишут
чанк и читают, не закрывая писателя, поэтому TIMEOUT.

## Направление починки (не предписание)

Отдавать инкрементально: держать нативный декодер между вызовами (или
скармливать накопленный буфер при каждом `transform` и отдавать прирост),
оставив `flush` только для завершения потока. Нативная сторона
(`_decompress_status_prefixed`, `dom.rs:205`) уже возвращает статус-байт,
которым отличается «пока не хватает данных» от «вход повреждён».

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   decompression-basic --variant decompression-formats` — ожидаются
   `ds-read … text=expected output` и оба `ds-gzip`/`ds-deflate-raw`.
2. WPT: `run_report.py --all --root compression` — семейство
   `decompression-*` должно перестать быть TIMEOUT.
