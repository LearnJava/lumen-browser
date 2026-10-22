# BUG-1404 — WOFF2-декодер принимает невалидные контейнеры, которые обязан отвергать (23 из 34 упавших WOFF2-тестов)

**Статус:** FIXED 2026-10-22 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** font (`crates/engine/font/src/woff2.rs::decode_woff2`, `:483`)

## Симптом

Временный тест `crates/engine/font/tests/` (удалён, в коммит не входит) прогнал `lumen_font::decode_woff2` по всем 299
файлам `tests/wpt/css/WOFF2/support/*.woff2`: **287 `Ok`, 12 `Err`**. Из 34 FAIL-тестов `css/WOFF2` декодер
принимает **23** — каждый из них по спецификации «MUST reject» (в `rel=help` тестов — `#conform-*-reject`,
`#conform-mustReject*`, `#conform-mustBeRejected-FailTransform`, `#conform-mustBeInvalidated-FailSize`):

| условие отказа | файлы |
|---|---|
| лишние нулевые байты между блоками/после них (`conform-extraneous-reject`) | `blocks-extraneous-data-003…008` |
| перекрытие блоков метаданных/приватных данных с данными таблиц (`conform-overlap-reject`) | `blocks-overlap-001…003` |
| `UIntBase128` > 2^32−1 | `datatypes-invalid-base128-002` |
| поле `length` заголовка на 4 байта короче/длиннее файла | `header-length-001`, `-002` |
| флаги преобразования `hmtx` (0 / все биты) | `tabledata-transform-hmtx-003`, `-004` |
| версия преобразования `head`/`glyf` | `tabledata-transform-bad-flag-001`, `-002` |
| `origLength` `loca` не равен расчётному / ненулевой преобразованный `loca` | `tabledata-bad-origlength-loca-001`, `-002`, `tabledata-non-zero-loca-001` |
| `origLength` первой таблицы уменьшена на 1 | `tabledata-decompressed-length-002` |
| `bbox` у пустого глифа / у составного без `bbox` | `tabledata-glyf-bbox-003`, `-002` |
| лишние данные перед последней таблицей | `tabledata-extraneous-data-001` |

Остальные 11 FAIL (`blocks-extraneous-data-001/002`, `directory-mismatched-tables-001`, `header-numTables-001`,
`header-signature-001`, `datatypes-invalid-base128-001/003`, `tabledata-brotli-001`,
`tabledata-decompressed-length-001/003/004`) декодер отвергает верно — они красные по другой причине
([BUG-1273](BUG-1273-FIXED.md): веб-шрифт не доходит до снимка).

## Как найдено

WPT-RUN-14 срез 18: `css/WOFF2` — 298 id, 264 зелёных, 34 FAIL. **Оговорка:** 264 PASS ненадёжны — под
`--screenshot` ни один `@font-face url()` не применяется (BUG-1273), тест и эталон рисуются запасным шрифтом и
совпадают. Эталон «PASS» (`WOFF Test CFF Reference`) и тест «F» различаются только тем, что отвергнутый шрифт
должен откатиться на запасной, а принятый — нарисовать «F». Поэтому 23 принятых контейнера доказаны зондом на
декодере, а не рендером; на рендере они проявятся, когда BUG-1273 будет закрыт.

## Что делать

Добавить в `decode_woff2` (`woff2.rs:483`) проверки отказа по W3C WOFF2 §5.1 (заголовок: `length`), §5.2
(директория: `UIntBase128`, порядок таблиц), §5.3/§5.4 (преобразования `glyf`/`loca`/`hmtx`) и §7 (блоки: отступы,
перекрытие); на каждое условие — юнит-тест на файле из `support/`.

## Как проверить

`css/WOFF2/blocks-overlap-001.xht`, `header-length-001.xht`, `tabledata-glyf-bbox-002.xht` после BUG-1273.

## Исправление (2026-10-22, P6)

`decode_woff2` отвергает все 23 контейнера из таблицы. Проверки (по образцу эталонного декодера Google, W3C WOFF2 §5):

- заголовок: `length` == размер файла;
- блоки: сжатый поток → метаданные → приватные данные идут вплотную с выравниванием до 4 байт, смещения `metaOffset`/`privOffset` совпадают с ожидаемыми, после последнего блока ничего нет (`blocks-extraneous-*`, `blocks-overlap-*`);
- `UIntBase128`: переполнение 2^32−1 проверяется до сдвига (прежняя граница 2^29−1 отвергала и допустимые значения);
- директория: преобразование допустимо только у `glyf`/`loca` (версия 0) и `hmtx` (версия 1); `glyf` и `loca` либо оба преобразованы, либо оба нет (`bad-flag-001/002`); `transformLength` у `loca` равен 0 (`non-zero-loca-001`);
- `origLength` `loca` == `(numGlyphs+1)·(2|4)` по `indexFormat` из заголовка `glyf` (`bad-origlength-loca-*`);
- сумма длин таблиц == размер распакованных данных (`decompressed-length-002`, `extraneous-data-001`);
- `glyf`: бит bbox у пустого глифа запрещён, у составного обязателен (`glyf-bbox-002/003`);
- `hmtx`: флаги с зарезервированными битами или оба бита lsb сброшены → отказ (`transform-hmtx-003/004`).

Проверено зондом по всем 299 файлам `tests/wpt/css/WOFF2/support`: 264 `Ok`, 35 `Err` (12 прежних + 23), ни один ранее принимавшийся валидный файл не отвергнут. Рендер-проверка по-прежнему ждёт BUG-1273.

Остаток: сама реконструкция преобразованного `hmtx` не реализована — [BUG-1678](BUG-1678-OPEN.md).
