# BUG-272 — ~1 ГБ RAM на lenta.ru при одной загруженной картинке (Edge целиком ~530 МБ)

**Статус:** OPEN — корень найден, срез 1 (пул offscreen-слоёв femtovg) влит 2026-07-07, срез 2 (blend-mode слой в пул) влит 2026-07-08, срез 3 (blend-result CPU-композит в пул) влит 2026-07-15; остаточные направления ниже
**Компонент:** paint (femtovg backend, offscreen-слои)
**Найден:** 2026-07-07, сравнительный замер Lumen vs Edge на lenta.ru
**Полная запись исследования (методика, опровергнутые гипотезы):** [docs/perf-audit-lenta-2026-07.md](../docs/perf-audit-lenta-2026-07.md)

## Симптом

lenta.ru в окне: 1371 МБ WS / 1124 МБ private при одной загруженной картинке. Baseline `samples/page.html`: 328/138 МБ. Edge на полной lenta.ru: ~530 МБ суммарно.

## Диагностика (2026-07-07)

Исключено замерами:
- **Известные кэши** — LUMEN_MEM_REPORT (env-гейт, остался в коде) показал: display-list cache 0.3 МБ, image cache ~0, prefetch 1.3 МБ, webfonts 1.1 МБ, QuickJS heap 13 МБ. Суммарно ~17 МБ.
- **Rust-куча целиком** — временный counting-allocator: живых аллокаций 119 МБ, пик 144 МБ (код удалён после замера).
- **Streaming-кадры** — file://-копия страницы (мгновенный HTML) даёт ту же память.
- **JS/DOM/layout/CPU-paint** — headless `--screenshot` той же страницы: пик 94 МБ.

Найдено: **GPU-память процесса = 1168 МБ** (`\GPU Process Memory(pid_*)\Local Usage`; интегрированная графика → это системная RAM, она и видна как private bytes).

## Корень

Каждый `PushClipRoundedRect` / `PushClipPath` / `PushOpacity` / `PushFilter` / `PushMask` / backdrop-элемент в femtovg-бэкенде делал `create_image_empty(width, height)` — offscreen-текстуру **размером со весь framebuffer** (~5 МБ при 1040×795 @1.25) — и ставил её в очередь `*_pending_delete`, освобождаемую только **после `canvas.flush()` в конце кадра**. На lenta.ru за кадр ~150 Push-команд → все ~150 текстур живы одновременно → ~750 МБ GPU-аллокаций за кадр; драйвер удерживает пик навсегда.

Синтетика: 120 блоков 50×20 с `border-radius+overflow:hidden` → **1025 МБ GPU / 918 МБ private**.

## Срез 1 — пул offscreen-слоёв (влит 2026-07-07)

`FemtovgBackend::layer_pool`: слой, освобождённый на Pop (`release_layer`), переиспользуется следующим Push (`acquire_layer`) в том же кадре — femtovg исполняет очередь команд строго по порядку, поэтому перезапись пикселей отпущенного слоя безопасна там, где удаление ImageId — нет. Пик слоёв = глубина вложенности (на lenta = 3), не число Push за кадр. Кап пула 8; ресайз окна ретирует пул через pending-delete. Одноразовые изображения (colour-matrix re-upload, blend PREMULTIPLIED, filtered backdrop) в пул не попадают.

| Замер | До | После среза 1 |
|---|---|---|
| Синтетика 120 клипов, GPU | 1025 МБ | 227 МБ (= пустое окно) |
| lenta.ru, GPU | 1168 МБ | 509 МБ |
| lenta.ru, WS / private | 1371 / 1124 МБ | 713 / 497 МБ |

Корректность: оконные тесты 03, 15 (blur shadow), 30 (filter), 31 (clip-path), 36 (radius), 101 (rounded clip), 103 (backdrop) — PASS либо ровно debtor-baseline; полный оконный прогон — без новых регрессий.

## Срез 2 — blend-mode offscreen-слой в пул (влит 2026-07-08)

`PushBlendMode` (все режимы кроме `Normal`/`PlusLighter`, которые уже были fast-path без offscreen) создавал `src_image_id` через `create_image_empty` напрямую, минуя `layer_pool` — единственный Push, оставшийся вне среза 1. Проверено: `src_image_id` используется только как render target и читается обратно через `screenshot()` (`composite_blend_layer`), никогда не сэмплится GPU-пейнтом (`Paint::image`) — поэтому флаг `FLIP_Y`, который несёт пул (в отличие от прежнего `PREMULTIPLIED`-only), для этого слоя не имеет значения; тот же довод, что уже применялся к blur-destination в `composite_filter_layer`. Теперь `acquire_layer()`/`release_layer()`, как у остальных Push-путей.

| Замер | До среза 2 | После среза 2 |
|---|---|---|
| Синтетика 120 `mix-blend-mode:multiply`, WS/private (первый кадр, file://) | 1230 / 976 МБ | 637 / 447 МБ |
| lenta.ru, WS/private (mix-blend-mode не используется на странице) | ~780 / ~565 МБ | ~734 / ~504 МБ (в пределах шума замера — сайт не задействует эту ветку) |

Корректность: юнит-тесты `lumen-paint` (828+29, 0 failed), clippy `-D warnings` чист, оконные тесты 03/15/30/31/36/101/103 — PASS либо ровно debtor-baseline, без регрессий.

**Новое наблюдение (не решено в этом срезе):** на синтетической странице с 120 blend-элементами WS/private продолжают расти в простое без единого взаимодействия (637→896 МБ WS за ~20 с), хотя event loop использует `ControlFlow::Wait`/`WaitUntil`, а не непрерывный `Poll`. На lenta.ru (без mix-blend-mode) такого роста нет. Вероятная связь — BUG-273: CPU-композитный результат блендинга (`result_id` в `composite_blend_layer`) создаётся заново через `create_image` из сырых пикселей на каждый `Pop` и остаётся one-off (не пулябелен — другой механизм создания, чем `create_image_empty`); если что-то периодически триггерит перерисовку этой страницы, каждый такой one-off может не отдаваться драйверу мгновенно. Причина периодической перерисовки не диагностирована. Требует отдельного расследования (LUMEN_FRAME_LOG=2 + профиль GPU-памяти во времени) — не блокирует этот срез, т.к. lenta.ru (реальный кейс бага) роста не показывает.

## Срез 3 — blend-result CPU-композит в пул (влит 2026-07-15)

Пункт 5 остатка: `composite_blend_layer` создавал `result_id` заново через `canvas.create_image()` (фреш GPU-загрузка сырых CPU-blend-пикселей) на каждый `Pop`, ставя его в `blend_layer_pending_delete` — не пулябелен `layer_pool`, т.к. этот image, в отличие от `src_image_id`, реально GPU-сэмплится (`Paint::image`), а `layer_pool` несёт `FLIP_Y` (только для render-target'ов, испортил бы сэмплинг).

Фикс: новый `blend_result_pool` (плоский `PREMULTIPLIED`-флаг, без `FLIP_Y`, как исходный `create_image`). Слот переиспользуется через `canvas.update_image(id, …, 0, 0)` вместо пересоздания. Безопасность reuse: `composite_blend_layer` всегда начинает с `canvas.flush()` — та же операция, что исполняет отложенный `fill_path` предыдущего blend-слоя (и его чтение пулового image) ДО того, как текущий вызов перезапишет тот же слот. Кап пула 4 (глубина вложенности blend-слоёв редко больше пары).

Корректность: `cargo test -p lumen-paint --features backend-femtovg` 924+29 passed, 0 failed; `cargo clippy -p lumen-paint --all-targets --features backend-femtovg -- -D warnings` чист; графический тест 56 (mix-blend-mode) — без регрессии.

Этот срез убирает one-off GPU-загрузку из «Нового наблюдения» (срез 2) как один из подозреваемых источников — периодическая перерисовка теперь переиспользует существующий image вместо накопления новых; причина самой периодической перерисовки (не диагностирована) остаётся за BUG-273.

## Остаток (следующие срезы)

1. ~~Blend-слои (PREMULTIPLIED) вне пула~~ — закрыто срезом 2 (src-слой) и срезом 3 (result-слой). Glyph atlas на тексте страницы — GPU на lenta всё ещё ~509 МБ против ~224 МБ baseline (~285 МБ страничных); требует того же счётчика GPU-памяти, срезы 2/3 его не меняли (lenta не использует blend-mode).
2. Baseline пустого окна 224 МБ GPU — сам по себе жирный (framebuffers/шрифтовой атлас/драйвер).
3. Слои по bounding box вместо full-frame — снизит и пул, и стоимость clear_rect.
4. Отложенные многокопийные image-кэши (см. диагностику 2026-07-07 в истории файла): femtovg `raw_images` deep-copy, `@WxH`-варианты, GIF все кадры, font `bytes_store.cloned()`, canvas2d thread-local — актуально для image-heavy сайтов.

## Инструменты (остались в коде)

`LUMEN_MEM_REPORT=1` — периодический дамп размеров хранилищ в stderr (`about_to_wait`); `RenderBackend::debug_mem_report()`; `QuickJsRuntime::debug_memory_used()`; `DecodedImageCache/PrefetchCache::debug_stats()`.
