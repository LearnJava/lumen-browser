# BUG-1150 — `<img crossorigin>` грузится заново на каждый элемент: `decode_image_cors` обходит `IMAGE_CACHE`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P6, при закрытии [BUG-1116](BUG-1116-FIXED.md) — остаток повторных GET на tradingview)
**Область:** shell (`crates/shell/src/subresources.rs::decode_image_cors` и его вызов в
`fetch_and_decode_images`; комментарий над функцией прямо называет дубль «acceptable for a first slice»)

## Симптом

`perf_audit.py --mode compat --only tradingview`, окно `--maximized`, ad-block выключен:
441 GET, из них 158 повторных; 155 — URL, которые в разметке стоят только в
`<img crossorigin src=…>` (логотипы `s3-symbol-logo.tradingview.com/*.svg`,
`earth-blurred.*.webp`; один и тот же логотип встречается в HTML 2-6 раз — desktop/mobile
варианты). Каждый элемент — отдельный сетевой запрос и отдельный декод
(`Загружена картинка: …earth-blurred… (2880×1537)` в логе дважды подряд). Chrome на той же
странице берёт каждый URL один раз (memory cache по URL + CORS mode + credentials).

## Корень

`IMAGE_CACHE` индексируется только URL. CORS-путь (`decode_image_cors`, GAP-CANVASORIGIN срез 2)
не может переиспользовать no-cors запись — она не прошла CORS-проверку — и поэтому не кэширует
вовсе, даже между двумя одинаковыми `crossorigin`-запросами.

## Что сделать

Отдельный слот кэша для CORS-результата с ключом `(URL, mode anonymous|use-credentials,
origin документа)` — как ключует memory cache HTML LS §2.5.5 «list of available images».
Одинаковые `<img crossorigin>` делят один запрос и один декод. Критерий: на tradingview нет
повторных GET по URL из `<img crossorigin>` в пределах одной навигации.
