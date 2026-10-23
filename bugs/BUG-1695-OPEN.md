# BUG-1695 — `TextDecoder`/`TextEncoder`: метки, `encodeInto`, потоковое декодирование, `replacement`-кодировки и IDL расходятся со спецификацией

**Статус:** OPEN
**Заведён:** 2026-10-23 (P2, WPT-ENC-SCOUT)
**Область:** js (`TextEncoder`/`TextDecoder` — `crates/js/src/dom.rs`, ~441, `_lumen_text_decode`), encoding (`crates/engine/encoding/src/lib.rs::from_label`)

## Симптом

Кластер F разведки (51 id категории `encoding`, 6 934 из 7 674 сабтестов уже проходят; остаток — перечисленные дефекты API, отдельные от отсутствия самих кодировок, см. [ENC-SINGLEBYTE](../ROADMAP.md)/ENC-CJK):

1. **Нормализация метки.** `from_label` вырезает **все** пробелы, а не обрезает ASCII-пробелы (U+0009/000A/000C/000D/0020) по краям. `new TextDecoder("\vutf-8")` и метки с пробелом внутри принимаются, хотя должны бросать `RangeError`: `api-invalid-label.any.html` — по 84 провала в каждом из 6 id (`?1-1000` 916/1000, `?3001-last` 337/421) — 504 сабтеста.
2. **`TextEncoder.encodeInto`.** Не бросает `TypeError` на `DataView`, `ArrayBuffer`/`SharedArrayBuffer` и отсоединённом буфере (`encodeInto.any.html` 84/111: `assert_throws_js … did not throw` ×24+); `textdecoder-copy.any.html` 1/2: после изменения исходного буфера `decode()` возвращает `""` вместо `"@"`.
3. **IDL (`idlharness.any.html` 31/57).** Интерфейсный объект `enumerable`; `length` конструктора 2 вместо 0; `prototype` writable; геттеры (`encoding`, `fatal`, `ignoreBOM`) на прототипе не бросают `TypeError`, свойства на прототипе отсутствуют (`The prototype object must have a property "encoding"`).
4. **Потоковый режим.** `textdecoder-streaming.any.html` 14/32 — потоковое `decode(chunk, {stream:true})` на границах чанков выдаёт `��…` вместо суррогатной пары (16 сабтестов) и `""` вместо `�` на хвосте (2); `textdecoder-fatal.any.html` 35/36 — `decode()` полной последовательности `♥` возвращает `""`; `textdecoder-byte-order-marks.any.html` 1/3 — `assert_not_equals: Mismatching BOM should not be ignored - treated as garbage bytes`.
5. **`replacement`-кодировки.** `replacement-encodings.any.html` 6/12: `csiso2022kr`, `hz-gb-2312` (и остальные метки `replacement`) декодируют вход как обычный текст вместо одного U+FFFD (`expected "U+FFFD" but got "U+0041/U+0042/…"`).
6. **BOM и sniffing.** `remove-only-one-bom.html` 0/3 (ожидали `"﻿"`, получили `"����"` ×2 и `"﻿﻿"` ×1 — срезается не ровно один BOM); `sniffing.html` 0/1 (ожидали `"Â€Â€ Hello World! Â€Â€"`, получили `" Hello World! "` — кодировка документа выбрана неверно).

## Проба

`python tests/wpt/run_corpus.py --binary <dev-release> --prefixes encoding/api-invalid-label encoding/encodeInto encoding/idlharness encoding/textdecoder-streaming encoding/replacement-encodings encoding/remove-only-one-bom`.

## Как найдено

WPT-ENC-SCOUT, кластер F (потолок +10.2 балла = 0.015 п.п. корпуса).

## Что делать

Исправлять по пунктам, каждый — со своим юнит-тестом в `crates/js`/`crates/engine/encoding`. Пункт 1 (504 сабтеста одной правкой в `from_label`) — первым. Метки отсутствующих кодировок (`iso-8859-*`, `windows-125x`, CJK) сюда не входят — это ENC-SINGLEBYTE/ENC-CJK.

## Как проверить

Перепрогон списка из «Проба»; ожидание — все перечисленные файлы выходят на 100 % сабтестов, кроме тех, что упираются в отсутствующие кодировки.
