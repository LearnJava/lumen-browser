# BUG-1235 — остаток BUG-1097: `script-tag`/`sharedworker-classic`/`a-tag` в `referrer-policy/4K*` и сквозная проверка BMP

**Статус:** OPEN (срез 1: BMP подтверждён, `[object Object]`-остаток закрыт; остаток 4K — BUG-1817/BUG-1818/BUG-1306, см. «Срез 1»)
**Тип:** не локализован / верификация.
**Заведён:** 2026-10-01 (P1, остаток [BUG-1097](BUG-1097-FIXED.md))
**Область:** WPT-инфраструктура `common/security-features/` (`requestViaScript`, `tests/wpt/common/security-features/resources/common.sub.js:626-635`).

## Что осталось

1. **Сквозная проверка BMP.** `lumen-image` теперь декодирует BMP (BUG-1097, срез 1), но
   `python tests/wpt/run_report.py --check --all --root referrer-policy/4K --recursive --processes 7`
   не запускался. Ожидание: 156 `img-tag`-подтестов переходят в unexpected PASS. После — регенерация
   baseline `referrer-policy` и `mixed-content` (и категорий, где `img-tag`/`picture-tag` берётся из
   `security-features`).
2. **Необъяснённый остаток.** `script-tag` 141, `sharedworker-classic` 12, `a-tag` 3 падают с
   `promise_test: Unhandled rejection with value: object "[object Object]"`. `script.py` отдаёт
   `application/javascript`, не изображение, — к BMP не относится. Кандидат: гонка
   `bindEvents2(window, "message", script, "error", window, "error")` в `requestViaScript` —
   возможно, наша доставка `postMessage`/событий ведёт себя иначе.

## Как проверить

Прогон из п. 1 + отдельный пробник на `requestViaScript` для п. 2 (процедура — `docs/probe-method.md`).

## Срез 1 (2026-11-15, P3) — BMP подтверждён, `[object Object]` объяснён и закрыт

Измерение: `run_report.py --binary target/dev-release/lumen.exe --all --root referrer-policy/4K
--recursive --processes 7` (Windows, 1:45): **96/108 harness OK, 168/392 сабтестов PASS**.

1. **BMP-декодер работает.** Проба на `image.py`: `<img>` получает `load`, `naturalWidth/Height`
   = 50×50, пиксели в `getImageData` совпадают с независимым разбором BMP-заголовка в JS
   (первые 4 пикселя 0/255/255, 255/255/0, 255/255/0, 0/0/255 в обоих). Ожидаемых «unexpected PASS»
   у `img-tag` не появилось не из-за BMP, а из-за двух других дефектов того же хелпера (п. 2, 3).
2. **`[object Object]`-остаток (п. 2 заявки) = `TextDecoder.decode` глотал не-`Uint8Array`.**
   `decodeImageData` (`common.sub.js:455`) передаёт `new TextDecoder("ascii").decode(new
   Uint8ClampedArray(...))` (это `ImageData.data`), а шим принимал только `Uint8Array` и молча
   подставлял пустой буфер — `TextDecoder` возвращал `""`, `JSON.parse("")` бросал
   `Unexpected end of JSON input`, reject уезжал в `promise_test` как `[object Object]`.
   Исправлено: `crates/js/src/shim/text_encoding_shim.js` принимает любой `ArrayBufferView`
   (и `ArrayBuffer`), иначе `TypeError` по WebIDL; тест
   `crates/js/tests/cases/bug1235_textdecoder_buffer_source.rs` (Uint8ClampedArray, DataView,
   ненулевой `byteOffset`). После правки во всём 4K **ноль** сабтестов с `[object Object]`
   (было 156), `script-tag` 40/64, `worker-classic`/`worker-module`/`sharedworker-classic` 12/24.
3. **Canvas taint в streaming-производителе.** `page_load.rs::spawn_image_requests` тянул картинку
   без CORS даже при `crossorigin`, а `apply_stream_intrinsic_sizes` красил канву по одному
   сравнению origin — 8 из 12 `img-tag` падали `SecurityError: The canvas has been tainted` на
   `getImageData`, хотя `image.py` отдаёт `Access-Control-Allow-Origin: *`. Исправлено:
   cross-origin + `crossorigin` идёт через существующий `decode_image_cors` (CORS-проверка; при
   провале пикселей нет вовсе), taint ставится только без атрибута. Остаются `IndexSizeError` —
   п. 4 ниже.

### Что осталось в 4K (224 сабтеста) — вне этого бага

- **172** — `Referer` не обновляется после `history.replaceState` → [BUG-1817](BUG-1817-FIXED.md).
- **52** `img-tag` (48 `IndexSizeError` + 4 с причиной из 1817) — `getImageData(0, 0,
  img.clientWidth, img.clientHeight)` сразу после `load` видит `clientWidth` 0 →
  [BUG-1306](BUG-1306-FIXED.md) (геометрия обновляется только следующим кадром).
- **12** файлов `a-tag` — TIMEOUT без сабтестов → [BUG-1818](BUG-1818-FIXED.md).

### Регенерация baseline не сделана

Пункт 1 заявки требует `--update-expected` по `referrer-policy`/`mixed-content`. Сейчас 224 из 392
сабтестов 4K красны по трём ещё не починенным причинам выше, поэтому регенерация закрепила бы их
как «ожидаемые». Делать после BUG-1817 и BUG-1306; BMP-часть для этого больше не блокер.

