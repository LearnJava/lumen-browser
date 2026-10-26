# BUG-1713 — `new TextDecoder('ascii')` бросает `RangeError`: метки семейства windows-1252 (`ascii`/`us-ascii`/`latin1`/`iso-8859-1`/`windows-1252`) не поддержаны — vkvideo.ru падает в «Что-то пошло не так», видео не открывается

**Статус:** FIXED 2026-10-26 (P6)
**Заведён:** 2026-10-26 (Chat, живая проверка vkvideo.ru по запросу пользователя)
**Область:** js + encoding (`crates/js/src/shim/text_encoding_shim.js:115` — `RangeError`; `crates/engine/encoding/src/lib.rs::Encoding::from_label` — нет `Windows1252`; `crates/engine/encoding/src/decoder.rs`, `tables.rs`)

## Симптом

`https://vkvideo.ru/video-213276384_456243633` (любая страница ролика): серверная разметка рисуется, затем SPA при первом React-рендере (`flushSync` → `render`) получает необработанное исключение

```
[JS error] Uncaught RangeError: Failed to construct 'TextDecoder': The encoding label provided ('ascii') is invalid.
```

и показывает собственный экран ошибки: `document.body.innerText` = «Поиск видео … Что-то пошло не так. Попробуйте обновить страницу. Обновить», `document.title` = «Group», ни одного `<video>` в DOM за 100 с, запросов к API ролика/плееру нет (только `catalog.getVideoShowcase`). Других JS-ошибок в stderr нет — это единственная причина.

По WHATWG Encoding §4.2 метки `ascii`, `us-ascii`, `ansi_x3.4-1968`, `latin1`, `l1`, `iso-8859-1`, `iso8859-1`, `iso88591`, `iso_8859-1`, `iso_8859-1:1987`, `iso-ir-100`, `csisolatin1`, `cp819`, `ibm819`, `cp1252`, `x-cp1252`, `windows-1252` обязаны давать кодировку `windows-1252`; Chrome/Firefox принимают их все (`new TextDecoder('ascii').encoding === 'windows-1252'`). В Lumen `Encoding::from_label` знает только UTF-8/16/32, windows-1251, KOI8-R, IBM866, поэтому `_lumen_text_encoding_for_label` возвращает `undefined` и конструктор бросает. Комментарий в шапке `text_encoding_shim.js` называет это «deliberate scope decision» — реальный сайт показывает, что для windows-1252 это решение ломает совместимость: метка `ascii` — частый приём для разбора бинарных форматов (fourcc в MP4/protobuf/заголовки).

## Проба

```js
new TextDecoder('ascii').encoding            // Chrome: "windows-1252"; Lumen: RangeError
new TextDecoder('latin1').decode(new Uint8Array([0x80, 0xE9]))  // Chrome: "€é"
```

Живая проба: `.tmp/vk_video_probe.py` (не в репозитории — MCP live window, `--maximized`, `LUMEN_NO_ADBLOCK=1`, без прокси), stderr — `[JS error] Uncaught RangeError …`.

## Что делать

1. `crates/engine/encoding`: добавить `Encoding::Windows1252` — таблица 128 кодовых точек для 0x80–0x9F/0xA0–0xFF (WHATWG `index-windows-1252.txt`; 0x80–0x9F — €‚ƒ„…†‡ˆ‰Š‹ŒŽ ‘’“”•–—˜™š›œžŸ с пятью неотображаемыми C1 0x81/0x8D/0x8F/0x90/0x9D, 0xA0–0xFF = U+00A0–U+00FF) рядом с `WIN1251` в `tables.rs`, ветка в `decoder.rs::decode*` через `decode_single_byte`, все метки выше в `from_label`, каноническое имя `windows-1252`.
2. `crates/js/src/v8_runtime.rs::text_encoding_for_label` / `_lumen_text_decode` — пропустить новое значение; `text_encoding_shim.js` — обновить шапку (windows-1252 теперь поддержан), `_lumenTextPendingTailLen` для однобайтовой кодировки возвращает 0, как для windows-1251.
3. Проверить, что `Encoding` используется и для декодирования HTML-документов (`detect.rs::sniff_meta_charset`, HTTP `charset=`): страница с `<meta charset="iso-8859-1">` теперь должна декодироваться как windows-1252, а не по прежнему фолбэку. Если это меняет вывод какой-либо страницы graphic tests — полный `python graphic_tests/run.py --continue-on-fail` и регенерация golden в том же коммите.
4. Регресс-тесты: юнит в `crates/engine/encoding` (`0x80` → `€`, `0xE9` → `é`, `0x81` → U+0081, все метки → `Windows1252`) и V8-тест в `crates/js/src/dom/tests/` (`new TextDecoder('ascii').encoding === 'windows-1252'`, decode `[0x80,0xE9]` → `"€é"`).
5. `docs/plan/tech-stack.md` (абзац про отказ от полного набора WHATWG), `CAPABILITIES.md` (строка про TextDecoder/кодировки, если есть) — синхронизировать.

## Как проверить

- `cargo test -p lumen-encoding` и targeted-тест TextDecoder в `lumen-js` зелёные, clippy чистый.
- WPT `encoding/textdecoder-labels.any.js` — подтесты для меток windows-1252 проходят (до фикса падают).
- Живая проверка (`--maximized`, `LUMEN_NO_ADBLOCK=1`, без прокси): `https://vkvideo.ru/video-213276384_456243633` — в stderr нет `RangeError … 'ascii'`, экрана «Что-то пошло не так» нет, в DOM появился `<video>` плеера. Если плеер появился, но ролик не играет (`readyState`, `error`, `play()`), — завести следующий баг с конкретной причиной (MSE/HLS/кодек), в этом баге не чинить.

## Исправление

- `lumen-encoding`: `Encoding::Windows1252` + таблица `WIN1252` (WHATWG index-windows-1252, байты C1 0x81/0x8D/0x8F/0x90/0x9D -> одноимённые U+0081…, не U+FFFD), все метки windows-1252 в `from_label`; `meta charset=iso-8859-1` теперь тоже даёт windows-1252.
- JS: отдельных правок рантайма не потребовалось (`text_encoding_for_label`/`text_decode` идут через `from_label`); обновлены комментарии шима.
- Тесты: юниты в `lumen-encoding` (tables/decoder/lib/detect), V8-тест `dom/tests/v8_bug1713_textdecoder_latin1.rs`.
- Живая проверка vkvideo.ru не выполнялась (проверка ролика — отдельно; при неработающем плеере заводить новый баг).
