# BUG-1084 — `TextDecoderStream` принимает не-`BufferSource` чанк: `writer.write('строка')` резолвится, а должен отклоняться `TypeError`

**Статус:** FIXED 2026-09-30 (P6)
**Тип:** дефект — нет проверки типа чанка в `transform` алгоритма `TextDecoderStream` (Encoding Standard, «decode and enqueue a chunk»).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** js — шим `TextDecoderStream` (`crates/js/src/shim/web_api_shim_*.js`)
**Владелец:** P3.

## Симптом

Проба страницы (`--dump-layout`, 2026-09-22): `new TextDecoderStream().writable.getWriter().write('not-a-buffer-source')` → промис **резолвится**.

WPT: `streams/decode-bad-chunks.any.html` 0/5 — все подтесты `assert_unreached: Should have rejected: write should reject` (5 сообщений).

## Ожидание

Чанк не `ArrayBuffer`/`ArrayBufferView` (включая `SharedArrayBuffer`-backed) → `write()` отклоняется `TypeError`, поток переходит в errored.

## Связанное

- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.

## Не проверялось

- Что именно передаёт каждый из пяти подтестов (`null`, `undefined`, число, объект…) — по логу видно только «должен был отклониться».

## Исправление

`transform` в `TextDecoderStream` (`crates/js/src/shim/streams_shim.js`) бросает `TypeError`, если чанк не `ArrayBuffer`/`SharedArrayBuffer`/view; view декодируется с учётом `byteOffset`/`byteLength`. Тест: `text_decoder_stream_rejects_non_buffer_source_chunk`.
