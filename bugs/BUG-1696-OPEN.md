# BUG-1696 — `TextEncoderStream`/`TextDecoderStream`: нет backpressure, ошибки transferred-чанка и `null` realm

**Статус:** OPEN
**Заведён:** 2026-10-23 (P2, WPT-ENC-SCOUT)
**Область:** js (`TextDecoderStream`/`TextEncoderStream` — `crates/js/src/dom.rs` ~409, потоки)

## Симптом

Кластер G разведки (30 id `encoding/streams/*`, 201 из 223 сабтестов проходят; остаток — четыре независимых дефекта):

1. **`backpressure.any.html` (0/4, по 3 варианта окружения).** Запись в `TextEncoderStream`/`TextDecoderStream` не ждёт чтения: `assert_array_equals: write should happen after read expected property 0 to be "paused"`, `writes should not happen before read2 …`.
2. **`decode-utf8.any.html` (10/12).** 2 сабтеста падают на `assert_array_equals: no chunks should be output lengths differ, expected array [] length 0` — на входе без полных символов (по разведке: transferred-чанк) выдаётся чанк.
3. **`invalid-realm.window.html` (0/4).** Все 4 сабтеста: `TypeError: Cannot read properties of null` — поток строится в realm iframe, который `null`.
4. **`realms.window.html` — TIMEOUT.** Harness не рапортует до таймаута (0/0); причина не установлена — вероятно, тот же чужой realm, что в п. 3.

(Метки неподдерживаемых кодировок в `decode-attributes`/`decode-non-utf8` — не здесь, это ENC-SINGLEBYTE/ENC-CJK.)

## Проба

`python tests/wpt/run_corpus.py --binary <dev-release> --prefixes encoding/streams`.

## Как найдено

WPT-ENC-SCOUT, кластер G (потолок +5.5 балла = 0.008 п.п. корпуса).

## Что делать

Привести `TextEncoderStream`/`TextDecoderStream` к Encoding §9.3/§9.4 (backpressure через стратегию очередей `TransformStream`); разобраться с чужим realm; затем перепрогнать `encoding/streams`.

## Как проверить

Перепрогон `encoding/streams`: `backpressure`, `decode-utf8`, `invalid-realm` — 100 %, `realms.window` — без TIMEOUT.
