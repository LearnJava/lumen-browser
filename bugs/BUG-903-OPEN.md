# BUG-903 — `parse_vtt`: блок не завершается на второй строке таймингов, а подпись `WEBVTT` принимает form feed

**Статус:** OPEN
**Заведён:** 2026-08-24 (P1, при закрытии [BUG-775](BUG-775-FIXED.md))
**Область:** `crates/engine/dom/src/vtt.rs::parse_vtt` — разбиение на блоки по пустым строкам (`for line in rest.split('\n')`) и проверка подписи (`header.as_bytes()[6].is_ascii_whitespace()`)
**Владелец:** P3

## Симптом

Шесть сабтестов `webvtt/parsing/file-parsing/`, ставших видимыми после
[BUG-775](BUG-775-FIXED.md) (до него весь каталог был TIMEOUT):

| тест | ожидается | получено |
|---|---|---|
| `arrows` | 6 cues | **0** |
| `timings, negative` | 4 cues | **0** |
| `nulls` | 7 cues | 4 |
| `whitespace chars` | 3 cues | 4 |
| `signature, formfeed` | `error` | `load` |
| `signature, two boms` | `error` | `load` |

## Причина

**1. Границы блока.** `parse_vtt` режет файл на блоки **только** по пустым
строкам, после чего у блока смотрит первую строку: если в ней нет `-->` —
считает её идентификатором cue, а строку под ней — таймингами. WebVTT §5
(«collect a WebVTT block») требует другого: блок завершается и на строке,
содержащей `-->`, если тайминги в этом блоке уже встречались.

`support/arrows.vtt` написан ровно под это правило — 14 строк подряд без единой
пустой:

```
-->
00:00:00.000 --> 00:00:01.000
text0
foo-->
00:00:00.000 --> 00:00:01.000
text1
…
```

По спеке это 6 cues; у нас это один блок, первая строка которого (`-->`)
содержит `-->` и потому берётся за строку таймингов — она невалидна, блок
отбрасывается целиком, результат 0 cues. `timings-negative.vtt` устроен так же.
`nulls`/`whitespace chars` расходятся на единицы по тому же корню.

**2. Подпись.** Проверка

```rust
header.starts_with("WEBVTT") && (header.len() == 6 || header.as_bytes()[6].is_ascii_whitespace())
```

принимает любой ASCII-пробельный байт, а `char::is_ascii_whitespace` в Rust
включает `\x0C` (form feed) и `\r`. WebVTT §4 разрешает после подписи ровно два
символа: U+0020 SPACE и U+0009 TAB. `support/signature-formfeed.vtt` — это байты
`57 45 42 56 54 54 0c 0a`, то есть `WEBVTT\x0C\n`, и он должен быть отвергнут.

**3. Двойной BOM.** `support/signature-two-boms.vtt` — `EF BB BF EF BB BF
"WEBVTT" 0A`. Спека разрешает снять **один** BOM. У нас их снимают
последовательно **двое**: сначала декодер тела ответа (`Response.text()`),
затем `parse_vtt` своим `strip_prefix('\u{FEFF}')` — и файл проходит. Чинить
надо не тем, что убрать одну из двух зачисток (обе на своём месте по
отдельности), а тем, чтобы `parse_vtt` не снимал BOM у строки, которую ему уже
отдал декодер; развилка не проработана.

## Как проверить фикс

`run_smoke.py /webvtt/parsing/file-parsing/tests/arrows.html` (0 → 1/1),
`/webvtt/parsing/file-parsing/signature-invalid.html` (9/11 → 11/11).
Юнит-тесты `parse_vtt` живут в `crates/engine/dom/src/vtt.rs`, туда же и
регрессии — фикс сетевого пути для этого не нужен.
