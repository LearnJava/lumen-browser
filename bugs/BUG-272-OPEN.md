# BUG-272 — ~1 ГБ RAM на lenta.ru при одной загруженной картинке (Edge целиком ~530 МБ)

**Статус:** OPEN — корень найден, срез 1 (пул offscreen-слоёв femtovg) влит 2026-07-07, срез 2 (blend-mode слой в пул) влит 2026-07-08, срез 3 (blend-result CPU-композит в пул) влит 2026-07-15, срез 4 (colour-matrix filter + backdrop-filter re-upload в общий пул) влит 2026-07-15, срез 5 (backdrop-filter → bbox-сайзинг вместо full-frame) влит 2026-07-16, срез 6 (шрифтовые байты через `Arc<[u8]>` — устранение двойного хранения @font-face-шрифта) влит 2026-07-18, срез 7 (`PushOpacity` несёт `bounds`, off-viewport opacity-группы куллятся) влит 2026-07-18, срез 8 (off-viewport clip-группы `PushClipRoundedRect`/`PushClipPath` куллятся) влит 2026-07-18, срез 9 (off-viewport mask-группы `PushMask{Image,LinearGradient,RadialGradient,ConicGradient}` куллятся) влит 2026-07-18, срез 10 (backdrop-filter's `elem_image_id` — bbox-сайзинг вместо full-frame) влит 2026-07-18, срез 11 (`PushOpacity` — bbox-сайзинг видимого слоя, общий `bbox_layer_pool`) влит 2026-07-18, срез 12 (`PushClipRoundedRect`/`PushClipPath` — bbox-сайзинг видимого слоя) влит 2026-07-18, срез 13 (`PushMask{LinearGradient,RadialGradient,ConicGradient}` — bbox-сайзинг видимого слоя) влит 2026-07-18, срез 14 (`PushFilter`/`PushBlendMode` — bbox-сайзинг видимого слоя) влит 2026-07-19, срез 17 (femtovg `raw_images` через `Arc<Image>`) влит 2026-07-19, срез 18 (`@WxH`-варианты femtovg — LRU-эвикция) влит 2026-07-19, срез 19 (анимированный GIF — ленивое декодирование кадров вместо eager `Vec<AnimatedFrame>`) влит 2026-07-19; остаточные направления ниже
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

## Срез 4 — colour-matrix filter + backdrop-filter re-upload в общий пул (влит 2026-07-15)

После среза 3 оставались ещё два one-off `create_image` того же класса (PREMULTIPLIED-only, без FLIP_Y, реально GPU-сэмплится через `Paint::image`, пересоздаётся на каждый Pop): (1) `composite_filter_layer`'s colour-matrix re-upload (не-blur/не-opacity `filter()` — grayscale/sepia/contrast/…) и (2) `apply_backdrop_filters`'s re-upload отфильтрованного backdrop-снимка для `backdrop-filter`. Оба структурно идентичны blend-result из среза 3 — те же флаги, тот же размер (framebuffer), тот же жизненный цикл (create → fill_path в этом же кадре → after-flush delete).

Фикс: `blend_result_pool`/`acquire_blend_result_image`/`release_blend_result_image` обобщены в `cpu_upload_pool`/`acquire_cpu_upload_image`/`release_cpu_upload_image` — общий пул на все три site'а. Безопасность разделяемого reuse: каждый потребитель (composite_blend_layer, composite_filter_layer's colour-matrix branch, apply_backdrop_filters — вызывается из PushBackdropFilter сразу после явного `canvas.flush()`) безусловно вызывает `canvas.flush()` непосредственно перед своим screenshot/re-upload шагом — тот самый flush исполняет отложенный `fill_path` любого более раннего потребителя (и его чтение того же слота) до того, как текущий вызов перезапишет слот через `update_image`. `BackdropFilterLayerEntry` получил поля `filtered_backdrop_w`/`filtered_backdrop_h` (нужны на Pop для `release_cpu_upload_image`). Поле `backdrop_filter_pending_delete` удалено — отфильтрованный backdrop теперь всегда возвращается в пул, а не удаляется безвозвратно.

Корректность: `cargo test -p lumen-paint --features backend-femtovg` 924+29 passed, 0 failed (тот же счёт, что и срез 3 — новых тестов не добавлено, поведение эквивалентно); `cargo clippy -p lumen-paint --all-targets --features backend-femtovg -- -D warnings` чист; оконные тесты 03/15/30/31/36/56/101/103 (в т.ч. 30 — CSS filter, 103 — backdrop-filter, прямые потребители этого среза) — без регрессии.

## Срез 5 — backdrop-filter: bbox-сайзинг вместо full-frame (влит 2026-07-16)

Первый шаг пункта 3 остатка («слои по bounding box вместо full-frame») — прошлая ветка `p3-bug-272-bbox-layers` была заведена под этот же пункт, но в итоге влила BUG-273 срез 1 (пивот на более узкую находку); пункт 3 оставался нетронутым до этого среза.

Выбран `apply_backdrop_filters`/`composite_backdrop_filter_layer` как самый безопасный старт: bbox (`bounds`, device px) уже вычислялся для клампинга окна blur-семплинга (`region`), поэтому финальный аплоад достаточно обрезать до этого же прямоугольника вместо загрузки всего framebuffer. `elem_image_id` (контент самого элемента, через общий `layer_pool`) в этом срезе не тронут — остаётся full-framebuffer, это отдельный будущий срез (требует смены семантики `layer_pool`, т.к. этот image GPU-сэмплится).

**Фикс:** новый хелпер `crop_region_rgba` вырезает `bounds`-прямоугольник (клампнутый к framebuffer) из CPU RGBA8-буфера после применения фильтров. Загружается только обрезанный кусок — новый выделенный пул `backdrop_bbox_pool` (не общий `cpu_upload_pool`: размер варьируется по элементам, шаринг одного пула с full-framebuffer-потребителями `cpu_upload_pool` вызывал бы вытеснение на каждом отличающемся по размеру backdrop-filter-слое). `BackdropFilterLayerEntry` несёт device-px origin кропа (`filtered_backdrop_x/y`), `composite_backdrop_filter_layer` мапит `Paint::image` на этот прямоугольник вместо `(0,0,css_w,css_h)`.

Корректность: `cargo test -p lumen-paint --features backend-femtovg` 929+29 passed, 0 failed (+3 юнит-теста на `crop_region_rgba`: базовая вырезка, пустой/вырожденный регион, клампинг к границам буфера); `cargo clippy -p lumen-paint --all-targets --features backend-femtovg -- -D warnings` чист. **Визуальная приёмка (2026-07-16, вторая сессия):** штатный gdigrab-гейт (`run.py --only 30`) на машине оказался неприменим — рабочий стол в момент прогона шёл через цветовую трансформацию (утренний прогон 04:40 видел чистую магенту `(255,0,255)`, вечерний — `(201,80,223)`: night-light/HDR-класс, десктоп-wide), из-за чего `is_magenta`-калибровка TEST-00 и любой абсолютный дифф против Edge ложно красные. Вместо этого — A/B-приёмка: одинаковые gdigrab-захваты живого femtovg-окна branch-бинаря против main-бинаря (трансформация искажает оба одинаково) на TEST-30 и TEST-103, кроп 1024×720 по рамке (ослабленный детектор в разовом скрипте, run.py не тронут). Результат: **побайтово идентично, 0/737280 отличающихся пикселей на обоих тестах** — срез 5 пиксельно нейтрален. femtovg больше не дефолтный оконный бэкенд (им стал wgpu, `P1-wgpu-flip`), так что риск этого среза и так ограничен `LUMEN_BACKEND=femtovg`-сессиями.

## Срез 6 — шрифтовые байты через `Arc<[u8]>` (влит 2026-07-18)

Первый шаг пункта 4 остатка (`font bytes_store.cloned()`, «отложенные многокопийные
кэши»). Каждый @font-face-шрифт хранился в памяти **дважды одновременно**: один раз в
`FontRegistry::bytes_store` (реестр провайдера) и ещё раз в `LoadedFace::bytes` рендера
(`crates/engine/paint/src/renderer.rs` — путь глифов wgpu/CPU, дефолтный бэкенд). Причина —
`FontProvider::read_face_bytes` возвращал `Option<Vec<u8>>`, т.е. `bytes_store.get(path).cloned()`
клонировал **весь буфер шрифта** на каждый вызов, а рендер складывал эту копию в `LoadedFace`.

**Фикс:** `bytes_store` хранит `Arc<[u8]>`; трейт `read_face_bytes` теперь возвращает
`Option<Arc<[u8]>>` (клон Arc = инкремент счётчика ссылок, буфер не копируется). `LoadedFace::bytes`
и слот воркера префетча (`FaceSlot`) — тоже `Arc<[u8]>`. Ключевой момент: байты @font-face в
`bytes_store` уже декодированы в sfnt (`register_from_bytes` кладёт результат WOFF/WOFF2-декода),
поэтому `maybe_decode_font` для них возвращает `Ok(None)` и рендер складывает **тот же самый Arc**,
что лежит в реестре — обе стороны разделяют одну аллокацию вместо двух копий. Дисковые (системные)
шрифты и WOFF-путь (`Ok(Some(decoded))`) получают свежий Arc, как раньше (нет второго хранилища —
дедуплицировать нечего). `FontRegistry::face_bytes_for_family` (shell-setup, вызывается раз на
@font-face-семью) оставлен с `Vec<u8>`-API (`to_vec()`) — там нет постоянного двойного хранения.

Экономия: одна полная копия каждого @font-face-шрифта на дефолтном рендер-пути (на реальных
сайтах с несколькими кастомными шрифтами — сотни КБ–единицы МБ). Дополнительно снят полный
клон буфера в воркере префетча (`bytes.clone()` → клон Arc).

Корректность: `cargo check -p lumen-paint` (default `backend-femtovg`) и `--features backend-femtovg`
зелёные; `cargo clippy -p lumen-core -p lumen-font -p lumen-paint --all-targets -- -D warnings` чист;
`cargo test -p lumen-font -- font_registry` 11/11 (в т.ч. новый `read_face_bytes_shares_allocation_across_calls`,
проверяющий `Arc::ptr_eq` двух чтений); `cargo test -p lumen-paint --lib -- face font` 33/33.

## Срез 7 — `PushOpacity` несёт `bounds`, off-viewport opacity-группы куллятся (влит 2026-07-18)

Первый шаг пункта «Остаток» 3c: `DisplayCommand::PushOpacity` теперь несёт не только
`alpha`, но и `bounds: Option<Rect>` — document-space CSS px bbox элемента, которому
принадлежит группа (та же конвенция, что у `PushBlendMode`/`PushFilter`/`PushBackdropFilter`).
Все четыре эмиттер-сайта (`box_layer_ops` opacity + isolate-reuse, оба SC-walk пути)
кладут `Some(b.rect)`; полностраничный view-transition fade в шелле (`vt_cmds`) — `None`
(нет bbox элемента → никогда не куллится).

Эффект: `FemtovgBackend::group_bounds` теперь возвращает bbox и для `PushOpacity`, поэтому
`run_content_pass` пропускает **весь** bracket `PushOpacity…PopOpacity` (acquire слоя, дети,
composite), когда bbox группы целиком вне вьюпорта — тот же механизм viewport-cull, что
BUG-273 срез 1 применил к blend-группам (`matching_close` уже балансирует opacity через
`overlay_partition::layer_delta`). opacity-группы — самый частый offscreen-класс, так что во
время скролла это снимает и full-framebuffer-аллокацию слоя, и CPU-композит для невидимых
opacity-поддеревьев. Bbox-сайзинг **видимого** слоя (аллокация слоя размером с bbox вместо
framebuffer) — отдельный будущий шаг того же пункта.

Корректность: `cargo check -p lumen-paint` (+`--features backend-femtovg`) и `-p lumen-shell`
зелёные; `cargo clippy -p lumen-paint --all-targets -- -D warnings` чист; `cargo test -p
lumen-paint --lib` 937/937 (в т.ч. обновлённый `group_bounds`-тест: `PushOpacity { bounds:
Some(r) }` → `Some(r)`, `bounds: None` → `None`). Хеш дисплей-листа фолдит `bounds` (hot-вариант
`hash_command_into` деструктурирует все поля).

## Срез 8 — off-viewport clip-группы куллятся (влит 2026-07-18)

Продолжение пункта «Остаток» 3c: `FemtovgBackend::group_bounds` теперь возвращает
bbox и для двух чистых clip-опенеров — `PushClipRoundedRect` (его `rect`) и
`PushClipPath` (`shape.bounding_rect()`) — поэтому `run_content_pass` пропускает
**весь** bracket `PushClip…PopClip`, когда его область целиком вне вьюпорта (тот же
механизм viewport-cull, что срезы 7 / BUG-273 срез 1; `overlay_partition::layer_delta`
уже балансирует clip-скобки, так что `matching_close` находит парный `PopClip`).

Clip — **самый безопасный** класс для этого куллинга: clip по определению ограничивает
каждого ребёнка своей областью, поэтому clip-rect/shape целиком вне вьюпорта ⇒ ни одного
видимого пикселя внутри (в отличие от opacity-групп, где нужно доверять тому, что
element-bbox покрывает детей). `rect` (rounded-clip) и вершины shape (basic-shape clip) —
ровно та геометрия, по которой рендер строит клип под текущей матрицей канвы, поэтому это
и есть корректный cull-bbox (та же координатная конвенция, что у групп выше). `PushClipRect`
(дешёвый scissor, без offscreen-слоя, эмиттером сегодня не выпускается) в срез не входит;
mask-опенеры (`PushMask*`) — тоже отдельный будущий срез (семантика их композита сложнее
чистого клипа).

Пассивная оптимизация только для `LUMEN_BACKEND=femtovg`-сессий (femtovg перестал быть
дефолтным оконным бэкендом после `P1-wgpu-flip`); дисплей-лист уже несёт нужные поля
(`rect`/`shape`), эмиттер не тронут.

Корректность: `cargo check -p lumen-paint` зелёный; `cargo clippy -p lumen-paint
--all-targets -- -D warnings` чист; `cargo test -p lumen-paint --lib` 937/937 (обновлённый
`group_bounds`-тест: `PushClipRoundedRect { rect: r }` → `Some(r)`, `PushClipPath { circle }`
→ `Some(bounding_rect)`).

## Срез 9 — off-viewport mask-группы куллятся (влит 2026-07-18)

Продолжение пункта «Остаток» 3c (последний оставшийся offscreen-опенер-класс):
`FemtovgBackend::group_bounds` теперь возвращает bbox и для всех четырёх mask-опенеров —
`PushMaskImage` / `PushMaskLinearGradient` / `PushMaskRadialGradient` /
`PushMaskConicGradient` — по их `rect` (border-box маскируемого элемента). `run_content_pass`
пропускает **весь** bracket `PushMask*…PopMask`, когда `rect` целиком вне вьюпорта (тот же
механизм, что срезы 7/8 / BUG-273 срез 1; `overlay_partition::layer_delta` уже балансирует
mask-скобки, включая вложенный `mask-clip` `PushClipRect`/`PopClip`, так что `matching_close`
находит парный `PopMask`).

Безопасность (mask ограничивает видимые пиксели своим `rect`, как clip): `PushMaskImage`
скиссорит маскируемое поддерево к `rect` (`canvas.scissor(rect…)` в `render_command`), поэтому
ни один пиксель не рисуется вне `rect`. Градиентные маски рендерят поддерево в offscreen-FBO,
а `composite_mask_layer` домножает его alpha через `CompositeOperation::DestinationIn`,
заливая градиент **только по `rect`** — вне `rect` источника нет, DestinationIn обнуляет там
alpha, так что композит вниз даёт видимые пиксели лишь внутри `rect`. Следовательно `rect`
целиком вне вьюпорта ⇒ ни одного видимого пикселя ⇒ cull всей группы корректен. `rect` — та же
координатная конвенция (document-space CSS px), что у clip/opacity/blend-групп выше.
`PushMaskLayer` (содержимое SVG-`<mask>`, применяется к **родительскому** слою) в срез не
входит — семантика его композита сложнее чистого клипа (как `PushClipRect` в срезе 8).

Пассивная оптимизация только для `LUMEN_BACKEND=femtovg`-сессий (femtovg перестал быть
дефолтным оконным бэкендом после `P1-wgpu-flip`); дисплей-лист уже несёт нужное поле
(`rect`), эмиттер не тронут.

Корректность: `cargo check -p lumen-paint` зелёный; `cargo clippy -p lumen-paint
--all-targets -- -D warnings` чист; `cargo test -p lumen-paint --lib` (обновлённый
`group_bounds`-тест: все четыре `PushMask*` с `rect: r` → `Some(r)`).

## Срез 10 — backdrop-filter's `elem_image_id`: bbox-сайзинг вместо full-frame (влит 2026-07-18)

Пункт 3(b) остатка (последний нетронутый full-framebuffer кусок backdrop-filter пути):
`elem_image_id` (содержимое самого элемента, `PushBackdropFilter`/`PopBackdropFilter`) сайзился
через `acquire_layer()` (полный framebuffer) — как и `filtered_backdrop_id` до среза 5. Новый
пул `elem_bbox_pool` (`acquire_elem_bbox_layer`/`release_elem_bbox_layer`, тот же паттерн, что
`backdrop_bbox_pool`, но render-target с `offscreen_layer_image_flags()` — FLIP_Y нужен, это
GPU-сэмплируемый слой) аллоцирует `elem_id` по тому же bbox, что `filt_id`
(`filtered_backdrop_w/h`).

Позиционная ловушка (нашлась не сразу — три ложных гипотезы по пути): у `elem_image_id`
и у `filtered_backdrop_id` **разные** конвенции координат. `apply_backdrop_filters`'s crop
индексирует скриншот через `bounds.x/y` напрямую, **без** поправки на ambient scroll/page-offset/
вложенные `PushTransform` — существующая (не эта срез) особенность срез-5-кода, вне скоупа
здесь. Композит шага 1 (`filtered_backdrop_id`) воспроизводит эту же (уже смещённую) позицию
через `reset_transform()` + сырой `bounds`. Старый (full-framebuffer) `elem_image_id`, наоборот,
рендерился с сохранённым ambient-трансформом (как любая другая команда рендерера) — то есть
оказывался на **истинной** экранной позиции элемента, и композитился как прямая копия всего
кадра, так что эта истинная позиция сохранялась без сдвига. Bbox-версия должна воспроизводить
именно это старое (корректное относительно себя) поведение `elem_image_id`, а не конвенцию шага 1.

Реализация: `PushBackdropFilter` захватывает `true_origin =
self.canvas.transform().transform_point(bounds.x, bounds.y)` **до** любых изменений
трансформа; при рендере детей в `elem_id` ambient-трансформ не сбрасывается (в отличие от
шага 1) — поверх него лишь добавляется `translate(-true_origin/scale)`, плюс
`canvas.scissor(bounds.x, bounds.y, bounds.width, bounds.height)` (переустановка, не
`intersect_scissor` — унаследованный scissor от предка, испечённый в старой системе координат,
иначе обрезает контент частично). `composite_backdrop_filter_layer` хранит `true_elem_x/y`
в `BackdropFilterLayerEntry` и композитит `elem_image_id` по этой позиции (`reset_transform()` +
`true_elem_x/y`, НЕ `filtered_backdrop_x/y` — иначе граница шага 1 «протекает» ровно в середину
контента элемента, разделяя карточку на видимо-по-разному-тонированные зоны).

Путь к диагнозу: A/B gdigrab (одна карточка backdrop-filter в изоляции) показал ту же нижнюю
границу артефакта независимо от FLIP_Y (вкл/выкл), пулинга (переиспользование/всегда-свежий) и
CPU-round-trip реаплоада вместо прямого GPU-сэмплинга — во всех трёх экспериментах байт-диф не
менялся. Корень нашёлся дампом `apply_backdrop_filters`'s CPU-кропа (`filtered_backdrop_id`'s
исходные пиксели) в PPM: побайтово идентичен между main и веткой (наполовину чёрный — сам по
себе отдельный, не влияющий на этот срез артефакт срез-5-кропа при ненулевом page-offset) — то
есть баг был не в шаге 1 и не в захвате `elem_image_id`, а исключительно в том, по какой позиции
срез 10 его композитил обратно.

Корректность: `cargo check -p lumen-paint` зелёный; `cargo clippy -p lumen-paint --all-targets
--features backend-femtovg -- -D warnings` чист; `cargo test -p lumen-paint --features
backend-femtovg` 937+29 passed, 0 failed. **Визуальная приёмка (A/B gdigrab
branch-vs-main, тот же метод, что срез 5 — штатный Edge-гейт недоступен, `run.py --only 30`
даёт `Edge screenshot missing` на этой машине):** одна изолированная карточка
(`.tmp/bd_minimal.html`, не коммитится) — 0/2949120 отличающихся байт; TEST-30 (все 5 карточек
backdrop-filter) — 0/2949120; TEST-103 (filter×transform, не задевает backdrop-filter) —
0/2949120; повторный прогон TEST-30/103 — тот же результат (детерминированно). Побайтовая
идентичность подтверждена дважды подряд для обеих сторон (main-vs-main и branch-vs-branch дают
0 диф — не флак gdigrab).

## Срез 11 — `PushOpacity`: bbox-сайзинг видимого слоя (влит 2026-07-18)

Пункт 3(c) остатка, первая часть. Срез 7 куллит off-viewport opacity-группы целиком, но
**видимый** (on-viewport) слой всё ещё аллоцировался full-framebuffer через `layer_pool`. Новый
общий пул `bbox_layer_pool` (переименован из среза-10-специфичного `elem_bbox_pool` —
`acquire_bbox_layer`/`release_bbox_layer`, тот же render-target-с-FLIP_Y паттерн, что использовал
`elem_image_id`) теперь разделяется между backdrop-filter's `elem_image_id` (срез 10) и
`PushOpacity`'s видимым слоем; срезы 12–14 переиспользуют его без изменений.

Новый метод `screen_bbox_device_px(local: Rect)` — bbox-сайзинг-аналог `is_command_culled`:
трансформирует 4 угла `bounds` (CSS px, pre-transform) текущей CTM (`self.canvas.transform()`),
берёт AABB, домножает на `self.scale` и клэмпит к `(self.width, self.height)` активного
render-таргета — возвращает device-px `(x0, y0, w, h)` вместо булева "виден/не виден". `None`
(вырожденный box или AABB схлопнулся в пустоту после клэмпа) → откат на full-framebuffer слой
через существующий `acquire_layer()`.

`PushOpacity` с `Some(bounds)`: аллоцирует `bbox_layer_pool`-слой размером `(w, h)`; дети рисуются
с сохранённым ambient-трансформом (та же конвенция, что срез-10's `true_elem_x/y` — **не**
ambient-blind конвенция `apply_backdrop_filters`'s кропа, см. доку `BackdropFilterLayerEntry`) плюс
доп. `translate(-x0/scale, -y0/scale)`, сдвигающим экранный origin bbox в локальный `(0, 0)` слоя;
следом переустанавливается `scissor(bounds.x, bounds.y, bounds.width, bounds.height)` — без этого
унаследованный от предка scissor (испечённый против старой CTM, до доп. translate) обрезал бы
контент координатами, принадлежащими полному framebuffer, а не этому маленькому слою (тот же
femtovg-капкан, что нашёл срез 10). `bounds` для `PushOpacity` — это собственный border-box
элемента (`b.rect`, без учёта overflow потомков) — та же конвенция, что уже принята срезами 7–9
для куллинга; отдельного регресса bbox-сайзинг не вносит.

`composite_opacity_layer` теперь ветвится по новому полю `OpacityLayerEntry::bbox`: bbox-путь
сначала `canvas.restore()` (снимает Push-time save+translate+scissor), затем переключает
render-таргет и композитит по `(x0/scale, y0/scale, w/scale, h/scale)` вместо `(0, 0, css_w,
css_h)`, освобождая слой через `release_bbox_layer(id, w, h)`; `None`-путь (full-framebuffer,
`bounds: None` — полностраничный view-transition fade среза 7, или откат при промахе bbox/пула)
не изменился.

Уточнение по системе координат femtovg (проверено чтением исходника `femtovg-0.9.2`,
`Canvas::set_size` кладёт `dpi` в отдельное поле `device_px_ratio`, а не в `state.transform`):
`canvas.transform()` — чистая композиция вызовов `translate`/`scale`/`rotate`, **без** встроенного
DPI-масштаба; значит `transform_point()` на CSS-px входе даёт CSS-px выход (та же система
координат, что `is_command_culled`'s сравнение с `cull_css_w/h`). Поэтому `screen_bbox_device_px`
домножает AABB на `self.scale` явно, а НЕ делит на него при последующем использовании (в отличие
от среза-10's `true_elem_x/y / self.scale` — вероятно, латентная неточность в срезе 10,
незамеченная т.к. headless-скриншоты рендерятся с `scale=1.0`; вне скоупа этого среза, не
трогалась).

Корректность: `cargo check -p lumen-paint` (default + `--features backend-femtovg`) зелёные;
`cargo clippy -p lumen-paint --all-targets --features backend-femtovg -- -D warnings` чист (и без
фичи); `cargo test -p lumen-paint --lib --features backend-femtovg` 937 passed, 0 failed, 2
ignored. Визуальная приёмка headless-скриншотами (`LUMEN_BACKEND=femtovg`, main-vs-branch,
`target/dev-release` main-бинарь собран раньше на этой машине): TEST-13 (visibility-opacity),
TEST-30 (css-filter), TEST-102 (opacity × z-index stacking, включает вложенный opacity 0.6×0.5 и
negative z-index внутри opacity-группы), `1000000-final.html` (полная демо-страница, opacity внутри
overflow:hidden-предков) — **все побайтово идентичны main** (`ImageChops.difference` bbox=`None`).
Синтетика (60 перекрывающихся полупрозрачных карточек 200×150, сетка 10×6) визуально корректна —
альфа-блендинг перекрытий без сдвигов/обрезаний.

## Срез 12 — `PushClipRoundedRect`/`PushClipPath`: bbox-сайзинг видимого слоя (влит 2026-07-18)

Пункт 3(c) остатка, вторая часть. Тот же механизм среза 11 (`screen_bbox_device_px` +
`acquire_bbox_layer`/`release_bbox_layer` через общий `bbox_layer_pool`), применённый к двум
clip-опенерам — той же bbox-геометрии, что уже используется для куллинга в срезе 8
(`group_bounds`'s `rect` для `PushClipRoundedRect`, `shape.bounding_rect()` для `PushClipPath`).

`ClipEntry::RoundedRectLayer`/`PathLayer` получили новое поле `bbox: Option<(f32, f32, usize,
usize)>` — тот же конвент, что `OpacityLayerEntry::bbox`. Push-обработчики пробуют
`screen_bbox_device_px` → `acquire_bbox_layer(w, h)`; при `None`/промахе аллокации откатываются на
существующий двухуровневый fallback (full-framebuffer `acquire_layer()`, а если и он промахнётся —
плоский прямоугольный scissor, BUG-132) — вынесен в отдельные `push_clip_rounded_rect_fallback`/
`push_clip_path_fallback`, чтобы не дублировать код на обеих ветках `match`.

Важное отличие от `PushOpacity`: `transform` (матрица канвы на момент Push, которой
`composite_clip_path_layer`/`composite_rounded_rect_clip_layer` строят экранный `path` формы клипа
для финального `fill_path` на identity-канве) захватывается **до** bbox-`translate`/`scissor`, а
не после — иначе путь клипа оказался бы в bbox-локальных, а не истинных screen-space координатах, и
`fill_path` на `prev_render_target` с identity-канвой рисовал бы форму не в том месте. `bbox`-путь
композита (`composite_clip_layer`) зеркалит `composite_opacity_layer`: сперва `canvas.restore()`
(снимает Push-time save+translate+scissor), переключение render-таргета, затем `Paint::image` по
`(x0/scale, y0/scale, w/scale, h/scale)` вместо `(0, 0, css_w, css_h)`, освобождение через
`release_bbox_layer`. Путь `path` не меняется между bbox/full-framebuffer веткой — он уже в
screen-space через `t`, а bbox по построению (`group_bounds`-геометрия) целиком накрывает всё, что
`path` может закрасить, так что сэмплирование `Paint::image` никогда не выходит за пределы слоя.

Корректность: `cargo check -p lumen-paint --features backend-femtovg` зелёный; `cargo test -p
lumen-paint --lib --features backend-femtovg` 937 passed, 0 failed, 2 ignored. Визуальная приёмка
headless-скриншотами (`LUMEN_BACKEND=femtovg`, main `target/dev-release` vs branch `target/release`,
собраны на этой машине): TEST-31 (clip-path), TEST-101 (rounded overflow), `1000000-final.html` —
**побайтово идентичны main**. Синтетика (rotate+scale rounded-rect, rotate clip-path polygon,
вложенный `overflow: hidden` со скруглением поверх повёрнутого контента, off-viewport
`clip-path: circle()` для проверки взаимодействия с куллингом среза 8, перекрывающиеся
circle/ellipse клипы с opacity) — также побайтово идентична main; визуально корректна (см. подробнее
`docs/tasks/bug-272-remaining-slices.md:54`).

## Срез 13 — `PushMask{Image,LinearGradient,RadialGradient,ConicGradient}`: bbox-сайзинг видимого слоя (влит 2026-07-18)

Пункт 3(c) остатка, третья часть. Тот же механизм срезов 11–12 (`screen_bbox_device_px` +
`acquire_bbox_layer`/`release_bbox_layer` через общий `bbox_layer_pool`), применённый к трём
gradient-mask-опенерам — той же bbox-геометрии (`rect`, масштаб-бокс маскируемого элемента), что уже
используется для куллинга в срезе 9. `PushMaskImage` из заголовка среза layer не открывает вовсе (нет
декодированной mask-текстуры на этом пути — approx через rect scissor, как и раньше) — тронуть было
нечего, менять только gradient-опенеры (`push_mask_gradient_layer`, единая точка для всех трёх).

`MaskLayerEntry` получила новое поле `bbox: Option<(f32, f32, usize, usize)>` — тот же конвент, что
`OpacityLayerEntry::bbox`/`ClipEntry::{PathLayer,RoundedRectLayer}::bbox`. `push_mask_gradient_layer`
пробует `screen_bbox_device_px` → `acquire_bbox_layer(w, h)`; при `None`/промахе аллокации
откатывается на существующий двухуровневый fallback (full-framebuffer `acquire_layer()`, а если и он
промахнётся — плоский прямоугольный scissor) — вынесен в `push_mask_gradient_fallback`, зеркалит
`push_clip_rounded_rect_fallback`.

Отличие от clip-срезов: `PopMask` (`composite_mask_layer`) должен сперва домножить alpha
offscreen-слоя на градиент (`DestinationIn`-заливка `rect`), и только потом композитить —
`fill_mask_gradient(&g, rect)` выполняется, пока Push-time `save()+translate` (bbox-путь) ещё не
откачен, поэтому `rect` красится в той же bbox-локальной системе координат, в которой рисовалась
маскируемая subtree — то же самое, что происходило в full-framebuffer-пути (там transform не
транслируется, ambient-канва совпадает с экранной). Затем `composite_mask_layer` передаёт `bbox`
дальше в `composite_opacity_layer` (уже bbox-aware с среза 11) вместо жёсткого `None` — тот
откатывает Push-time `save()+translate+scissor` через `canvas.restore()`, переключает render-таргет и
композитит `Paint::image` по `(x0/scale, y0/scale, w/scale, h/scale)` вместо `(0, 0, css_w, css_h)`.

Корректность: `cargo check -p lumen-paint --features backend-femtovg` зелёный; `cargo test -p
lumen-paint --lib --features backend-femtovg` 937 passed, 0 failed, 2 ignored;
`cargo clippy -p lumen-paint --features backend-femtovg --all-targets -- -D warnings` зелёный.
Визуальная приёмка headless-скриншотами (`--screenshot`, main vs branch, оба на `dev-release`
профиле, собраны на этой машине): TEST-26 (mask-image: linear/radial gradient mask, control,
mask-mode alpha/luminance), TEST-104 (mask × gradients × radius interaction) — **побайтово идентичны
main** (`PIL.ImageChops.difference` bbox = `None` на обоих). Edge headless-скриншот в этой
sandbox-среде не запускается вовсе (`msedge.exe --headless --screenshot=...` возвращает пустой вывод
без создания файла что на main, что на ветке — окружение, не регрессия), поэтому `run.py`'s
Edge-vs-Lumen путь недоступен; A/B main-vs-branch выше — эквивалентная замена для этого среза.

## Срез 14 — `PushFilter`/`PushBlendMode`: bbox-сайзинг видимого слоя (влит 2026-07-19)

Пункт 3(c) остатка, последняя часть. `PushBlendMode` (все режимы кроме `normal`/`plus-lighter`,
fast-path без offscreen-слоя) переведён на тот же механизм срезов 11–13 безусловно — mix-blend это
чистый попиксельный CPU-бленд (`mix_blend_rgba`), без чтения соседних текселей, тот же довод, что
уже применялся к clip/mask. `PushFilter` разделён на два случая по наличию `blur()` в цепочке:
colour-matrix-only цепочка (grayscale/sepia/brightness/invert/contrast/saturate/hue-rotate, без
blur) — тот же безопасный bbox-путь; цепочка с `blur(sigma>0)` **остаётся full-framebuffer
безусловно** — `filter_image`'s GPU Gaussian blur сэмплирует тексели за пределами border box
элемента, а `bounds` не несёт запаса под `~3σ` (ни один эмиттер `PushFilter`/box-shadow/text-shadow
его не добавляет); срез 11's перенос push-time save+translate в bbox-local пространство устраняет
причину исходного провала BUG-076/BUG-145 (позиционирование), но не проблему недостающего
blur-контекста за краем bbox — оставлено full-frame как явный, задокументированный компромисс,
а не забытый TODO.

Новый обобщённый пул `bbox_cpu_upload_pool`/`acquire_bbox_cpu_upload_image`/
`release_bbox_cpu_upload_image` — тот же принцип, что `bbox_layer_pool` уже применяет к
render-target-слоям (одна категория переменного-по-группе размера, несколько типов-потребителей):
делит слот между `composite_filter_layer`'s colour-matrix-only bbox re-upload и
`composite_blend_layer`'s blend-result bbox re-upload, отдельно от full-framebuffer-сайзинг
`cpu_upload_pool` (оставленного этим же двум потребителям на их full-frame fallback-путях —
blur-цепочка `PushFilter`, промах bbox-аллокации). Общий helper `upload_to_pool` (акквайр-агностичный
update_image-с-фолбэком-на-create_image) убирает дублирование между двумя потребителями (третье
место того же паттерна после `apply_backdrop_filters`).

`composite_blend_layer`'s bbox-композит переиспользует `CompositeOperation::Copy` с прямоугольником
`(x0/scale, y0/scale, w/scale, h/scale)` вместо full-canvas quad — GL блендит только реально
растеризуемые пикселя примитива, так что пиксели вне bbox остаются нетронутыми (тот же довод, что
уже применялся к `composite_clip_layer`'s bbox-пути, просто с другим composite-режимом).
`FilterLayerEntry`/`BlendLayerEntry` получили поле `bbox: Option<(f32, f32, usize, usize)>` — тот
же конвент, что `OpacityLayerEntry`/`ClipEntry::*`/`MaskLayerEntry`.

Корректность: `cargo check`/`cargo clippy -p lumen-paint --all-targets --features backend-femtovg
-- -D warnings` зелёные; `cargo test -p lumen-paint --features backend-femtovg` 937 passed, 0
failed, 2 ignored (тот же счёт, что срезы 12/13 — новых тестов не добавлено, существующие покрывают
геометрию через `group_bounds`/`screen_bbox_device_px` юнит-тесты). Визуальная приёмка — A/B
gdigrab (`LUMEN_BACKEND=femtovg`, branch vs чистый merge-base `27b30624`, оба dev-release,
собраны в одном worktree для инкрементальной пересборки; Edge headless в этой sandbox-среде
недоступен, тот же вынужденный метод, что срезы 5/12/13): **TEST-30 (`css-filter`, вкл.
backdrop-filter-ряд) — 0.135% (визуально пустой diff, порог 0.5% не задет)**, colour-matrix-only
bbox-путь корректен по всем 8 цветовым фильтрам + hue-rotate, blur-ряд (full-frame, нетронут)
идентичен. **TEST-56 (`mix-blend-mode`) — 12.767% diff branch-vs-baseline**, но расследование
(временный `eprintln!` в `composite_blend_layer`, снят после диагностики) показало: baseline сам по
себе **не рендерит вообще никакой** не-`normal`/`plus-lighter` blend-режим (`src_rgba.len() !=
backdrop_rgba.len()` в `composite_blend_layer` из-за существующего, несвязанного бага —
`acquire_layer()`/`layer_pool` сайзят full-frame слой по `self.width`/`self.height` вместо размера
активного band-render-таргета при scroll-blit-рендере, заведено отдельно как
[BUG-320](../BUGS.md)). Срез 14's bbox-путь для `PushBlendMode` не завязан на `self.width/height` и
случайно обходит этот баг — branch корректно рендерит все 17 ячеек TEST-56 (визуально проверено),
diff отражает **фикс плохого baseline**, не регрессию от этого среза.

## Срез 15 — `PushMaskLayer` (SVG `<mask>` content): viewport-cull — исследовательский, фикса нет

Пункт 3(c) остатка, последний класс опенера. Отличие от срезов 7–9: `PushMaskLayer`/
`PopMaskLayer` — не самодостаточная скобка (в отличие от `PushMask{Image,Gradient}`, которые
скиссорят/композитят **своё собственное** поддерево внутри `rect`). Как явно задокументировано
в `DisplayCommand::PushMaskLayer`: команды между `PushMaskLayer`/`PopMaskLayer` рендерят
**содержимое маски** в отдельный offscreen-слой, а `PopMaskLayer` применяет его как множитель
к **родительскому** слою (`parent_pixel *= mask_value(mask_layer_pixel, mode)`), ограниченный
`rect`. Композит `PopMaskLayer` не трогает пиксели вне `rect` — то есть чисто с точки зрения
композит-семантики off-viewport `rect` ⇒ композит невидим, тот же довод, что уже применялся
к `PushMask*` в срезе 9.

Проверка боем показала, что вопрос не в семантике композита, а в том, что куллить нечего:

1. `PushMaskLayer`/`PopMaskLayer` **не эмитируются нигде в продакшен-коде**. Единственный
   эмиттер mask-команд — `emit_push_mask` (`display_list.rs`) — покрывает только
   `PushMaskImage`/`PushMaskLinearGradient`/`PushMaskRadialGradient`/`PushMaskConicGradient`
   (по `b.style.mask_image`); ветки на `PushMaskLayer` там нет. Репозиторий-wide grep
   (`PushMaskLayer {` конструкторы) подтверждает: единственные места, где команда
   **строится**, — юнит-тесты `display_list.rs` (не реальный контент). SVG `<mask>` с
   произвольным rendered-содержимым (в отличие от `mask-image: url()`/градиента) — заявленная
   в докстринге, но не подключённая end-to-end фича CSS Masking L1 §5.
2. Как следствие, у двух бэкендов **разная** (и в одном случае — неверная относительно
   докстринга) реализация одной и той же команды, ни разу не пройденная реальным рендером:
   `renderer.rs` (wgpu) реализует полную семантику (отдельный offscreen-уровень
   `mask_layer_stack`, `MaskLayerComposite`-план, множительный композит по `rect`);
   `femtovg_backend.rs`'s `PushMaskLayer`/`PopMaskLayer` (`render_command`, строки ~4966–4976)
   — это просто `canvas.save()` + `canvas.scissor(rect)` + `canvas.restore()`, т.е. никакого
   отдельного mask-слоя и множительного композита нет вовсе, что противоречит докстрингу
   и семантике, которую wgpu-путь действительно исполняет.
3. Ни `graphic_tests/COVERAGE.md`, ни `TESTS` в `graphic_tests/run.py` не содержат отдельного
   теста на этот путь (маски покрыты через `mask-image`/градиенты, срез 9 и раньше).

Вывод: безопасный viewport-cull для `PushMaskLayer` семантически возможен (тот же аргумент
среза 9 — композит ограничен `rect`), но добавлять его сейчас означало бы писать код куллинга
поверх недостижимого в продакшене, ни разу не отрендеренного end-to-end пути с уже разошедшейся
между бэкендами (и одним из них — некорректной относительно докстринга) семантикой; нет ни
одной реальной страницы или графического теста, способных подтвердить, что cull ничего не
ломает. Это противоречит принципу проекта «не писать код под гипотетические future
requirements». Пункт остаётся **открытым до тех пор, пока не появится реальный эмиттер**
SVG-`<mask>`-контента (отдельная, более крупная задача — подключить произвольное поддерево как
источник маски, а не просто зафиксировать существующие Push/Pop-обработчики) — на этом этапе
тот же bbox-cull-паттерн срезов 7–9, вероятнее всего, ляжет без переработки. Femtovg-бэкенда
несоответствие докстрингу (п. 2 выше) зафиксировано здесь как находка, вне скоупа фикса этого
среза — фиксить нечего рендерить, чинить стаб под несуществующий вызывающий код не имеет
смысла.

Корректность: изменений в коде нет (чисто исследовательский срез, как и предусмотрено брифом
`docs/tasks/bug-272-remaining-slices.md:77-85`); `cargo check -p lumen-paint` не запускался
повторно — код не менялся.

## Срез 16 — GPU-счётчик по категориям + расследование glyph atlas (~285 МБ) — гипотеза опровергнута

Пункт 1 остатка. `RenderBackend::debug_mem_report()`/`LUMEN_MEM_REPORT` до этого среза печатал
только счётчики записей в пулах (`layer_pool=N`), не их GPU-байты, и не имел вообще никакого
доступа к femtovg-glyph-atlas — единственному кандидату на «страничные» 285 МБ, не объяснённые
известными Rust-хранилищами (см. п. 1 остатка). Новый `FemtovgBackend::gpu_image_bytes` суммирует
`canvas.image_size(id).0 * .1 * 4` по набору `ImageId` — точно, не оценочно: каждая текстура,
которую этот бэкенд создаёт, фиксированно `PixelFormat::Rgba8` (и `create_image_empty`-сайты, и
`ImageSource::Rgba`-загрузки), так что множитель ×4 байт/пиксель не приближение. Glyph atlas стал
измеримым через `femtovg::Canvas::debug_inspector_get_font_textures()` — метод, доступный только
под cargo-фичей `debug_inspector` крейта `femtovg` (добавлена в `crates/engine/paint/Cargo.toml`,
`features = []`, никаких новых транзитивных зависимостей); без неё `TextContext`, владеющий
атласом, полностью приватен снаружи крейта femtovg, и получить даже количество текстур атласа
было невозможно. Framebuffer/swapchain — не femtovg `ImageId` вообще (им владеет glutin/OpenGL),
поэтому это одно-буферная RGBA8-оценка по известному размеру поверхности (`width×height×4`), а не
измеренное значение — помечено `≈` в отчёте.

**Результат на реальном lenta.ru** (`python scripts/mem_perf.py --page https://lenta.ru --tabs 2
--tab-dwell-s 14 --hold-s 15`, `LUMEN_BACKEND=femtovg`, живое окно, `dev-release`):

```
femtovg: raw_images=1 (0.2 MB), gpu_images=1 (0.2 MB), glyph_atlas=1 (1.0 MB),
framebuffer≈3.1 MB, layer_pool=2 (6.2 MB), content_band_pool=0 (0.0 MB),
cpu_upload_pool=0 (0.0 MB), backdrop_bbox_pool=0 (0.0 MB), bbox_layer_pool=1 (2.9 MB),
bbox_cpu_upload_pool=0 (0.0 MB)
```

**Glyph-atlas-гипотеза опровергнута.** Один атлас = ровно одна страница 512×512 RGBA8 = 1.0 МБ —
femtovg переиспользует existующие страницы для глифов любого шрифта/размера/начертания через
общий rect-packer (`Atlas::add_rect`, не привязан к конкретному фонту), новая страница заводится
только когда все текущие исчерпаны по месту. Чтобы объяснить 285 МБ атласом, странице пришлось бы
разрастись до ~285 страниц (~285 × ~1800 упакованных глифов ≈ пол-миллиона одновременно живых
глифов) — не наблюдается ни на одной реальной странице, включая lenta.ru. Все femtovg-владеемые
GPU-текстуры вместе (raw_images + gpu_images + glyph_atlas + framebuffer-оценка + все layer-пулы)
суммарно дают **~13.6 МБ** против ~285 МБ необъяснённого прироста из исходной находки BUG-272 —
т.е. **источник не в каком-либо femtovg `ImageId`, за которым может уследить этот бэкенд вообще**.
Проверка на синтетике (4000 абзацев случайного текста, чередование bold/italic/regular,
`.tmp/heavy_text_static.html`, вне репозитория — не коммитится) дала тот же результат: `glyph_
atlas=1 (1.0 MB)` — согласуется с рассуждением про rect-packer выше, не эффект малого корпуса
lenta.ru конкретно.

Вывод: неатрибуцированный GPU-прирост, зафиксированный в исходной находке BUG-272 (и подтверждённый
независимо в PERF-5, `docs/perf/memory.md`: GPU ≈ 60% RSS на синтетике с 6 вкладками), лежит **вне
зоны видимости femtovg-бэкенда** — вероятные кандидаты: OpenGL-драйверные внутренние аллокации
(shader/command-buffer кэши), реальная (>1, в отличие от однобуферной оценки этого среза)
многобуферность swapchain/window-поверхности, либо особенность метода измерения (`Get-Counter
'GPU Process Memory\Local Usage'` может задваивать разделяемую системную память на интегрированной
графике — см. `docs/perf/memory.md`, "gpu_mb показан отдельно и не вычтен"). Дальнейшее
расследование этой части упирается не в отсутствие инструмента (он теперь есть и покрывает всё,
чем управляет паблик API `femtovg::Canvas`), а в то, что причина, скорее всего, ниже уровня
абстракции, доступного этому крейту — нужен либо драйверный/OS-level профайлер (вне скоупа Lumen),
либо переход на бэкенд с прямым контролем аллокаций (wgpu, уже дефолтный для windowed-режима,
ADR-017 — задача для отдельного среза/бага, не завершения этого).

Проверено: `cargo check -p lumen-paint --features backend-femtovg`, `cargo clippy -p lumen-paint
--features backend-femtovg --all-targets -- -D warnings`, `cargo test -p lumen-paint --features
backend-femtovg --lib` (937 passed) — все зелёные. Живая проверка (`mem_perf.py`) на
`samples/page.html` (`glyph_atlas=1 (1.0 MB)`, `content_band_pool=1 (14.6 MB)`, остальные пулы
0 — страница без opacity/clip/mask/blend, ожидаемо), на синтетике (см. выше) и на lenta.ru (см.
выше) — числа во всех трёх случаях разумны и внутренне согласованы (glyph atlas не растёт с
объёмом текста, пока не исчерпана packer-ёмкость страницы).

## Срез 17 — Multi-copy image cache: `raw_images` deep-copy (femtovg) — дедуплицировано через `Arc`

Пункт 4 остатка, часть 1. `FemtovgBackend::register_image` делал `self.raw_images.insert(src,
image.clone())` — **полную копию декодированных пикселей** каждой картинки помимо той, что уже
живёт за `Arc<Image>` в глобальном `IMAGE_CACHE` (`DecodedImageCache`) и в CPU
`ImageDecodeCache` (`self.image_cache`). Дедуплицировано тем же приёмом, что срез 6 применил к
шрифтовым байтам (`Arc<[u8]>` вместо `Vec<u8>`-клона):

- `RenderBackend::register_image` теперь принимает `Arc<Image>`, а не `&Image` (трейт
  `crates/engine/paint/src/backend.rs`); `FemtovgBackend::raw_images` — `HashMap<String,
  Arc<Image>>`, регистрация клонирует **указатель**, разделяя аллокацию пикселей с вызывающей
  стороной вместо второго экземпляра. Все пять реализаций трейта обновлены (femtovg, wgpu-обёртка,
  cpu, vello, compare); wgpu `Renderer::register_image` (inherent-метод) оставлен на `&Image` —
  он держит CPU-копию только под kill-switch'ем `LUMEN_NO_IMAGE_MIPS` (дефолтный mip-путь читает
  GPU-текстуру напрямую), поэтому `WgpuBackend` разыменовывает `Arc`.
- Shell протянут end-to-end: `fetch_and_decode_images` возвращает `Arc<Image>` — в горячей точке
  `DecodedImage::Static(img)` теперь `Arc::clone` хэндла кэша вместо `(*img).clone()`, так что
  `page.images` разделяет аллокацию с `IMAGE_CACHE`, а `register_image` — с обоими. Поля
  `ParsedPage`/`LoadedPage`/`PageSnapshot::pending_images` несут `Arc<Image>`. Streaming/lazy-пути
  используют то, что `ImageDecodeCache::insert` возвращает `ImageHandle` (`Arc<Image>`): вставляют
  в CPU-кэш, затем регистрируют возвращённый хэндл — `raw_images` разделяет аллокацию **CPU-кэша**
  (ноль лишних копий). Одноразовые пути (canvas-snapshot, GIF-кадры, bg-картинки) оборачивают
  свежий декод в `Arc::new` один раз.
- `render_to_image_cpu`/`rasterize_cpu` (headless CPU-путь) тоже приняли `&[(String, Arc<Image>)]`
  (`img.as_ref()` при построении `image_map`). Драйверный `RenderedPage.images` оставлен owned
  `Image` — оборачивается в `Arc::new` на границе shell'а, драйверный путь не тронут.

Экономия: на главном пути загрузки (`page.images`) все три места — `IMAGE_CACHE`, `page.images`,
`raw_images` — теперь разделяют один буфер вместо трёх копий; femtovg `raw_images` больше **не**
держит собственный экземпляр. Изменение пиксельно-нейтрально по построению: везде, где рендер
читает пиксели, `Arc<Image>` разыменовывается в тот же `&Image` (femtovg `register_image` —
`image_to_rgba8_vec(&image)`, cpu_raster — `img.as_ref()`), — подтверждено попиксельно: headless
`--screenshot` страницы с шестью `<img>` (`samples/test-04-images.html`, PNG+JPEG) на ветке vs
main **побайтово идентичен** (`PIL.ImageChops.difference` bbox `None`), картинки рендерятся (не
grey placeholder).

Проверено: `cargo check`/`clippy -p lumen-paint` (backend-femtovg, backend-vello+compare,
backend-cpu+cpu-render) и `-p lumen-shell`/`-p lumen-driver` — зелёные; `cargo test -p lumen-paint
--features backend-femtovg --lib` (937 passed), `--features backend-cpu,cpu-render --lib
cpu_raster` (62 passed, включая `draw_image_paints_decoded_pixels`/`draw_cross_fade`).

## Срез 18 — Multi-copy image cache: `@WxH`-варианты — LRU-эвикция (femtovg)

Пункт 4 остатка, часть 2. Оценка: femtovg держит по одной полноценной GPU-текстуре на каждый
различный placed-размер картинки (area-averaged downscale под ключом `"src@WxH"`, BUG-077).

**Дедупликация уже сделана и дальше невозможна без потери качества.** Ключ
`format!("{src}@{tw}x{th}")` уже даёт *попадание* при точном совпадении `(source, target_w,
target_h)` — два `<img>` одного источника в одинаковом placed-размере делят один вариант. Разные
размеры — это честно разные пересэмплированные пиксели: свести их к одному буферу без повторного
ресемплинга нельзя, а ресемплинг вернул бы именно тот алиасинг, ради устранения которого варианты
существуют. Поэтому по формулировке среза («либо LRU-эвикцию, если дедупликация невозможна без
потери качества») реализован **рычаг эвикции**, а не дальнейшая дедупликация.

**Проблема, которую закрывает срез:** до этого варианты копились в общем `self.images` **без
границы** — весь `"src@WxH"`-зоопарк жил до следующей навигации (`clear_images`). Одна картинка,
переразмещаемая во множестве размеров за сессию (responsive-relayout, зум, анимация), накапливала
неограниченно много осиротевших текстур.

**Сделано (только femtovg-бэкенд):**

- Варианты вынесены из `self.images` (там остались только оригиналы, ключ = голый `src`,
  жизненный цикл register→clear) в отдельный `resized_variants: LruMap<femtovg::ImageId>` —
  небольшой обобщённый LRU-кэш фиксированной ёмкости (`RESIZED_VARIANT_CACHE_CAP = 128`): `map` +
  вектор `order` (LRU в начале), `get` помечает ключ недавним, `insert` сверх ёмкости вытесняет
  начало и возвращает вытесненный `ImageId`. Вытесненная текстура отправляется в
  `resized_variant_pending_delete` и удаляется после следующего `canvas.flush()` (как остальные
  pending-delete-очереди — вытесненный id мог быть отрисован ранее в этом же кадре).
  `clear_images` дренирует новый кэш и очередь; `debug_mem_report` (срез 16) получил отдельную
  категорию `resized_variants`.
- Ёмкость 128 взята с большим запасом относительно числа различных placed-размеров, видимых в
  одном кадре, — внутрикадрового thrash'а нет, эвикция ограничивает лишь межкадровое накопление.

**Пиксельная нейтральность — по построению.** Ключ, функция ресемпла (`resize_area_avg` +
`image_to_rgba8_vec` + `create_image`) и семантика lookup для незаполненного кэша идентичны
прежним; эвикция срабатывает только после накопления 128 различных вариантов (много больше, чем
видимо в кадре) и лишь освобождает устаревший вариант, который при повторной надобности
пересэмплируется идентично. `@WxH`-путь femtovg живёт только в живом окне (headless `--screenshot`
— CPU-путь, его не задевает), поэтому визуальная приёмка = юнит-тесты логики LRU
(`lru_map_*`, 5 шт.: hit/miss, порядок вытеснения, обновление недавности, drain, пол ёмкости) +
полный femtovg lib-прогон без регрессий.

**Область.** wgpu-бэкенд (дефолтный, ADR-017) `@WxH`-зоопарка **не имеет**: по умолчанию грузит
оригинал с mip-цепочкой (одна текстура на `src`, downscale делает трилинейный сэмплер), а
`"src@WxH"`-путь там — только под kill-switch'ем `LUMEN_NO_IMAGE_MIPS=1` (legacy, редкий).
Поэтому срез сделан для femtovg (всегда-активный путь, как и срезы 10–17); legacy-wgpu-путь
оставлен как есть — он bounded уже тем, что почти не используется.

Проверено: `cargo check`/`clippy -p lumen-paint --features backend-femtovg --all-targets -D
warnings` — зелёные; `cargo test -p lumen-paint --features backend-femtovg --lib` — 942 passed
(937 прежних + 5 новых `lru_map_*`), 2 ignored, 0 failed.

## Срез 19 — Multi-copy image cache: GIF все кадры — ленивое декодирование (image + shell)

Пункт 4 остатка, часть 3. Оценка: анимированный GIF раскодировался eager'ом — `decode_gif_animated`
разворачивал **все** кадры в `Vec<AnimatedFrame>`, каждый — полный экранный RGBA-буфер
`width × height × 4`. Для многокадрового крупного GIF это `N × width × height × 4` резидентно, хотя
для плавного воспроизведения в каждый момент нужен ровно один кадр.

**Сделано (ленивое декодирование, `crates/engine/image/src/gif.rs`):** `AnimatedGif` больше не
хранит пиксели. Хранятся только закодированные байты (`encoded: Arc<[u8]>`, разделяемые между
клонами) и per-frame задержки (`delays_cs: Vec<u16>` — дешёвые метаданные, собираются один раз при
загрузке проходным декодом в один переиспользуемый отбрасываемый буфер, пиковая память прохода —
один кадр). Пиксели кадра декодируются по запросу через `frame_image(idx)`: forward-курсор
(`GifCursor` за `Mutex` → `Send + Sync`) держит живой `gif::Decoder` спозиционированным, так что
воспроизведение вперёд стоит **один декод кадра на переход**, а не `O(N)`; запрос кадра «позади»
курсора (wrap на 0 в цикле, обратный seek) пересоздаёт декодер с начала; повтор того же кадра
обслуживается из кэша последнего кадра. Резидентная память = закодированные байты + ~один кадр.

**Мотивация формы.** GIF-кадры взаимозависимы (disposal composited поверх предыдущих) — произвольный
доступ к кадру `N` в принципе требует последовательного декода `0..=N`, поэтому «декодировать окно»
свелось к forward-курсору с пересозданием на откат: он даёт `O(1)`-амортизацию на forward-плейбэке
(доминирующий сценарий: `frame_index_at` монотонно растёт, wrap на 0 раз в цикл) при резидентной
границе в один кадр.

**API-переход (shell).** Публичное поле `frames: Vec<AnimatedFrame>` убрано; сайты в
`crates/shell/src/main.rs`/`image_cache.rs` переведены на методы: `frame_count()`,
`frame_image(idx) -> Result<Image>`, `total_cycle_ms()`, `frame_delay_ms(idx)`, `resident_bytes()`
(диагностика `LUMEN_MEM_REPORT` теперь считает реальный footprint, а не `N`-кадровый eager).
Первый кадр (`frame_image(0)`) по-прежнему материализуется сразу для начальной отрисовки. Бонус:
два прежних `(*gif).clone()` (финальный pipeline + streaming-loader) больше не копируют пиксели всех
кадров — клон делит `Arc<[u8]>` и копирует лишь `delays_cs`. `AnimatedFrame` удалён из публичного API.

**Пиксельная нейтральность — по построению.** `frame_image(idx)` воспроизводит ровно ту же
последовательность операций декодера (`ColorOutput::RGBA`, `next_frame_info` + `read_into_buffer`
кадр за кадром с внутренним composite/disposal), что и прежний eager-цикл до кадра `idx`, — байты
кадра `idx` идентичны. Проверка: `cargo test -p lumen-image` — 32 passed (включая новые
`lazy_frame_pixels_match_source`, `lazy_backward_access_resets_cursor`, `lazy_out_of_range_clamps`,
`lazy_metadata_decoded_without_pixels`, `animated_gif_is_send_sync` — round-trip на синтетическом
2-кадровом GIF через `gif::Encoder`); `clippy -p lumen-image`/`-p lumen-shell -D warnings` — зелёные.

## Остаток (следующие срезы)

1. ~~Blend-слои (PREMULTIPLIED) вне пула~~ — закрыто срезами 2–4 (src-слой, blend-result, colour-matrix filter, backdrop-filter — все теперь в едином `cpu_upload_pool`). ~~Glyph atlas на тексте страницы~~ — **срез 16 опроверг гипотезу**: новый GPU-байтовый счётчик по категориям (`debug_mem_report`, femtovg `debug_inspector` feature) показал glyph atlas = 1.0 МБ (одна страница 512×512) на реальном lenta.ru, все femtovg-владеемые текстуры вместе ~13.6 МБ — источник ~285 МБ неатрибуцированного GPU лежит вне видимости femtovg-бэкенда (драйвер/swapchain/метод измерения, см. срез 16 выше); не блокирует переход к следующим пунктам.
2. Baseline пустого окна 224 МБ GPU — сам по себе жирный (framebuffers/шрифтовой атлас/драйвер).
3. Слои по bounding box вместо full-frame — **срез 5 (влит) сделал backdrop-filter's `filtered_backdrop_id` bbox-сайзингом**; визуально подтверждён (A/B gdigrab branch-vs-main, TEST-30/103 побайтово идентичны — см. срез 5 выше); **срез 7 (влит) добавил `bounds` в `PushOpacity` и включил viewport-cull off-screen opacity-групп** (см. срез 7 выше); **срез 8 (влит) включил viewport-cull off-screen clip-групп** (`PushClipRoundedRect`/`PushClipPath`, см. срез 8 ниже); **срез 9 (влит) включил viewport-cull off-screen mask-групп** (`PushMask{Image,LinearGradient,RadialGradient,ConicGradient}`, см. срез 9 ниже); **срез 10 (влит) сделал backdrop-filter's `elem_image_id` bbox-сайзингом** (пункт (b), см. срез 10 выше); **срез 11 (влит) сделал `PushOpacity`'s видимый слой bbox-сайзингом** через общий `bbox_layer_pool` (пункт (c), см. срез 11 выше); **срез 12 (влит) сделал `PushClipRoundedRect`/`PushClipPath`'s видимый слой bbox-сайзингом** тем же механизмом (пункт (c, продолжение), см. срез 12 выше); **срез 13 (влит) сделал gradient-mask-опенеров (`PushMask{LinearGradient,RadialGradient,ConicGradient}`) видимый слой bbox-сайзингом** тем же механизмом (`PushMaskImage` слой не открывает — тронуть было нечего, см. срез 13 выше); **срез 14 (влит) сделал `PushBlendMode`'s (безусловно) и `PushFilter`'s colour-matrix-only (без blur) видимый слой bbox-сайзингом** через новый общий `bbox_cpu_upload_pool` (пункт (c), последний, см. срез 14 выше); `PushFilter`'s blur-цепочка осознанно оставлена full-framebuffer (нет запаса под GPU-blur-сэмплинг за краем bbox — см. срез 14); **срез 15 (исследовательский, фикса нет)** установил, что `PushMaskLayer` (SVG-`<mask>` content) не эмитируется нигде в продакшене — семантически cull возможен (тот же довод, что срез 9), но писать его сейчас не над чем проверить; пункт остаётся открытым до появления реального эмиттера SVG-`<mask>`-контента (см. срез 15 выше).
4. Отложенные многокопийные image-кэши (см. диагностику 2026-07-07 в истории файла): ~~femtovg `raw_images` deep-copy~~ (закрыто срезом 17 — трейт `register_image` принимает `Arc<Image>`, `raw_images` разделяет аллокацию с `IMAGE_CACHE`/CPU-кэшем, см. срез 17 выше), ~~`@WxH`-варианты~~ (закрыто срезом 18 — дедуп по `(source,tw,th)` уже был ключом `"src@WxH"`, дальше без потери качества невозможен; femtovg-варианты вынесены в LRU-ограниченный `resized_variants` (cap 128), wgpu-дефолт `@WxH`-зоопарка не имеет — mip-цепочка, см. срез 18 выше), ~~GIF все кадры~~ (закрыто срезом 19 — `AnimatedGif` больше не держит `N` кадров: только `Arc<[u8]>`-байты + `delays_cs`, кадры декодируются лениво через forward-курсор с резидентной границей ~один кадр, см. срез 19 выше), ~~font `bytes_store.cloned()`~~ (закрыто срезом 6 — `Arc<[u8]>`), canvas2d thread-local (срез 20) — актуально для image-heavy сайтов.

## Инструменты (остались в коде)

`LUMEN_MEM_REPORT=1` — периодический дамп размеров хранилищ в stderr (`about_to_wait`); `RenderBackend::debug_mem_report()` (срез 16: GPU-байты по категориям — raw_images/gpu_images/glyph_atlas/framebuffer/каждый layer-пул — не только счётчики записей); `QuickJsRuntime::debug_memory_used()`; `DecodedImageCache/PrefetchCache::debug_stats()`.
