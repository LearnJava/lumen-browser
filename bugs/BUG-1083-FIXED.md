# BUG-1083 — `TextDecoder` в потоковом режиме не срезает BOM, если его байты разнесены по чанкам: `decode({stream:true})` + `decode()` и `TextDecoderStream`

**Статус:** OPEN
**Тип:** дефект — состояние «BOM уже видели» выставляется по первому вызову, а не по первому реально декодированному символу (`subsystems/js.md`: `_pending`/`_sawInput`, при `_sawInput` на следующих вызовах натив получает `ignoreBOM=true`).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** js — оболочка `TextDecoder` в шиме (`crates/js/src/shim/web_api_shim_*.js`), не натив `_lumen_text_decode`
**Владелец:** P3.

## Симптом

Проба страницы (`--dump-layout`, 2026-09-22):

- `d = new TextDecoder(); d.decode(new Uint8Array([0xEF,0xBB]), {stream:true})` → `""`, затем `d.decode(new Uint8Array([0xBF,0x40]))` → `"﻿@"`; ожидается `"@"`. Так же при делении 1+3.
- `TextDecoderStream` (utf-8, `ignoreBOM:false`), два `write` с делением BOM: делёж 0/1/2 → `"﻿abc"`, только делёж 3 (BOM целиком в первом чанке) даёт `"abc"`; для `utf-16le`
  (`FF FE` …) — то же при делении 0/1.

WPT: `streams/decode-ignore-bom.any.html` 5/12 (7 × `BOM should be stripped expected "abc" but got "﻿abc"`), `textdecoder-copy.any.html` 0/2 (одно из двух — `expected "@" but got "﻿@"`;
второе — [BUG-1085](BUG-1085-OPEN.md)), `textdecoder-byte-order-marks.any.html` 1/3 (не разбирался — сообщение про «mismatching BOM», возможно другая причина).

## Ожидание

Encoding Standard, decode: BOM снимается, если он — начало потока; «BOM seen» ставится, когда декодер выдал первый символ / обработал первые три (UTF-8) или два (UTF-16) байта, а не по факту вызова.

## Связанное

- `subsystems/js.md` — абзац «`TextDecoder`'s streaming/BOM state lives entirely in the shim» (BUG-357).
- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.
