# BUG-1082 — `TextEncoder`/`TextEncoderStream` кодируют одиночный суррогат в WTF-8 (`ED A0 80`), а не в U+FFFD (`EF BF BD`); `TextEncoderStream` не переносит старший суррогат между чанками

**Статус:** OPEN
**Тип:** дефект — нарушение алгоритма «convert code units to scalar values» (Encoding Standard, `TextEncoder.encode`, `TextEncoderStream`).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** js — шим `TextEncoder`/`TextEncoderStream` (`crates/js/src/shim/web_api_shim_*.js`, где именно — не искалось)
**Владелец:** P3.

## Симптом

Проба страницы (`--dump-layout`, 2026-09-22):

- `new TextEncoder().encode('\ud800')` → `237,160,128` (`ED A0 80`); по спецификации `239,191,189` (`EF BF BD`).
- `TextEncoderStream`: `write('\ud83d'); write('\udca9'); write('a')` → чанки `[237,160,189]`, `[237,178,169]`, `[97]`; по спецификации пара, разорванная границей чанков,
  склеивается в один `F0 9F 92 A9`, одиночный — заменяется на `EF BF BD`.

WPT (baseline `encoding`, окно): `api-surrogates-utf8.any.html` 1/6, `textencoder-utf16-surrogates.any.html` 2/7, `streams/encode-utf8.any.html` 3/19
(12 сообщений `number of chunks should match expected`, 3 — `assert_array_equals` по байтам). Что именно каждый подтест `encode-utf8` проверяет — по одному не сверялось;
проба выше согласуется с обоими классами сообщений.

## Ожидание

`TextEncoder.encode` заменяет каждый одиночный суррогат на U+FFFD до UTF-8-кодирования; `TextEncoderStream` держит «pending high surrogate» между чанками.

## Связанное

- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.

## Не проверялось

- `TextEncoder.encodeInto` с суррогатами (`encodeInto.any.html` 30/111 — смешано с [BUG-1085](BUG-1085-OPEN.md), 54 подтеста упираются в `SharedArrayBuffer`).
