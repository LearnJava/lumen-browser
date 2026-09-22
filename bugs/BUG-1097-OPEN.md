# BUG-1097 — `lumen-image` не умеет BMP: `<img>`/`<picture>` на общий WPT-хелпер `security-features/subresource/image.py` всегда падает с `error`

**Статус:** OPEN (ДОРАБОТКА)
**Тип:** нереализованная функциональность — `lumen-image` разбирает 8 форматов по сигнатуре (`crates/engine/image/src/lib.rs:120-152`: PNG/JPEG/GIF/WebP/AVIF/SVG/JXL/HEIC), BMP среди них нет
**Область:** image (`crates/engine/image/src/lib.rs::decode_raw`), затрагивает WPT-инфраструктуру `common/security-features/subresource/image.py` (используется `img-tag`/`picture-tag` во ВСЕХ категориях `security-features`, не только `referrer-policy`)
**Владелец:** P1 (`lumen-image`)
**Заведён:** 2026-09-22 (WPT-RUN-7 срез 54, `referrer-policy/4K` baseline)

## Симптом

Любой WPT-тест, использующий общий хелпер `requestViaImage`/`requestViaPicture`
(`tests/wpt/common/security-features/resources/common.sub.js:402-408`,
`crossOrigin: "Anonymous"` + steganography-декодирование через canvas),
падает с `promise_test: Unhandled rejection with value: object "[object Object]"` —
`<img>`'s `error`-событие вместо `load`, независимо от same-origin/
cross-origin и от типа редиректа (воспроизводится и на
`same-http origin and no-redirect redirection`, то есть не CORS и не
редирект).

## Причина

`common/security-features/subresource/image.py:106` отдаёт
`content_type = b'image/bmp'` (payload — самодельный минимальный BMP-энкодер,
`image.py:10-64`, ради steganography-кодирования ответных заголовков в
пиксели без зависимости от PIL). `lumen-image::decode_raw`
(`crates/engine/image/src/lib.rs:120-152`) проверяет сигнатуры PNG/JPEG/GIF/
WebP/AVIF/SVG/JXL/HEIC по очереди и **не содержит ветки для BMP** (`BM`
magic bytes, 14-byte `BITMAPFILEHEADER` + `BITMAPINFOHEADER`) — падает в
`Err(ImageError::UnknownFormat)` (`lib.rs:152`), пробрасывается через
`fetch_and_decode_images`/`decode_image` в `subresources.rs` как обычная
ошибка декодирования → `<img>` получает `error`, не `load`.

## Прямое измерение

`tests/wpt/referrer-policy/4K` (WPT-RUN-7 срез 54, тот же прогон, что
BUG-1096): **312 подтестов** только в семье `4K*` падают с этим сообщением —
`img-tag` 156, `script-tag` 141 (другой, не подтверждённый механизм — не
`image.py`, см. «Остаток» ниже), `sharedworker-classic` 12, `a-tag` 3.
Из 156 `img-tag`-подтестов причина подтверждена исходным кодом
(`decode_raw` не знает BMP), не только косвенно по симптому.

## Масштаб за пределами этого среза

`image.py`/`requestViaImage` — общая инфраструктура `common/security-features/`,
используемая категориями `mixed-content`, `content-security-policy`,
`upgrade-insecure-requests`, `referrer-policy`, `cross-origin-resource-policy`
и другими (везде, где тест выбирает `img-tag`/`picture-tag` как один из
типов сабресурса). Масштаб по остальным категориям этим срезом не измерен —
`referrer-policy` был первой, где источник ошибки разобран до сигнатуры
формата.

## Остаток

`script-tag`/`sharedworker-classic`/`a-tag` (156 подтестов той же `[object Object]`
сигнатуры) — НЕ объясняются этой причиной (`script.py` отдаёт
`application/javascript`, не изображение). Механизм не установлен в рамках
этого среза — кандидат на отдельный пробник (`requestViaScript`'s
`bindEvents2(window, "message", script, "error", window, "error")`,
`common.sub.js:626-635` — гонка между `window`'s "message" и глобальным
"error", возможно наше `postMessage`/событийная доставка тут ведёт себя
иначе).

## Направление починки (не предписание)

Добавить BMP-декодер в `lumen-image` (сигнатура `BM`, `BITMAPFILEHEADER`/
`BITMAPINFOHEADER`, минимум — uncompressed 24-bit, ровно то, что пишет
`image.py`'s самодельный энкодер) по образцу существующих модулей
(`gif.rs`/`webp` — отдельный файл/модуль, не встраивать в `lib.rs`).

## Как проверить фикс

`tests/wpt/run_report.py --check --all --root referrer-policy/4K --recursive --processes 7` —
156 `img-tag`-подтестов этого кластера должны перейти в unexpected PASS.
