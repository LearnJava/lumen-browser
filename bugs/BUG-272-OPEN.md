# BUG-272 — ~1 ГБ RAM на lenta.ru при одной загруженной картинке (Edge целиком ~530 МБ)

**Статус:** OPEN — корень найден, срез 1 (пул offscreen-слоёв femtovg) влит 2026-07-07, срез 2 (blend-mode слой в пул) влит 2026-07-08, срез 3 (blend-result CPU-композит в пул) влит 2026-07-15, срез 4 (colour-matrix filter + backdrop-filter re-upload в общий пул) влит 2026-07-15, срез 5 (backdrop-filter → bbox-сайзинг вместо full-frame) влит 2026-07-16, срез 6 (шрифтовые байты через `Arc<[u8]>` — устранение двойного хранения @font-face-шрифта) влит 2026-07-18, срез 7 (`PushOpacity` несёт `bounds`, off-viewport opacity-группы куллятся) влит 2026-07-18, срез 8 (off-viewport clip-группы `PushClipRoundedRect`/`PushClipPath` куллятся) влит 2026-07-18, срез 9 (off-viewport mask-группы `PushMask{Image,LinearGradient,RadialGradient,ConicGradient}` куллятся) влит 2026-07-18, срез 10 (backdrop-filter's `elem_image_id` — bbox-сайзинг вместо full-frame) влит 2026-07-18, срез 11 (`PushOpacity` — bbox-сайзинг видимого слоя, общий `bbox_layer_pool`) влит 2026-07-18; остаточные направления ниже
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

## Остаток (следующие срезы)

1. ~~Blend-слои (PREMULTIPLIED) вне пула~~ — закрыто срезами 2–4 (src-слой, blend-result, colour-matrix filter, backdrop-filter — все теперь в едином `cpu_upload_pool`). Glyph atlas на тексте страницы — GPU на lenta всё ещё ~509 МБ против ~224 МБ baseline (~285 МБ страничных); требует того же счётчика GPU-памяти, срезы 2–4 его не меняли (lenta не использует blend-mode/backdrop-filter/цветовые CSS-фильтры).
2. Baseline пустого окна 224 МБ GPU — сам по себе жирный (framebuffers/шрифтовой атлас/драйвер).
3. Слои по bounding box вместо full-frame — **срез 5 (влит) сделал backdrop-filter's `filtered_backdrop_id` bbox-сайзингом**; визуально подтверждён (A/B gdigrab branch-vs-main, TEST-30/103 побайтово идентичны — см. срез 5 выше); **срез 7 (влит) добавил `bounds` в `PushOpacity` и включил viewport-cull off-screen opacity-групп** (см. срез 7 выше); **срез 8 (влит) включил viewport-cull off-screen clip-групп** (`PushClipRoundedRect`/`PushClipPath`, см. срез 8 ниже); **срез 9 (влит) включил viewport-cull off-screen mask-групп** (`PushMask{Image,LinearGradient,RadialGradient,ConicGradient}`, см. срез 9 ниже); **срез 10 (влит) сделал backdrop-filter's `elem_image_id` bbox-сайзингом** (пункт (b), см. срез 10 выше); **срез 11 (влит) сделал `PushOpacity`'s видимый слой bbox-сайзингом** через общий `bbox_layer_pool` (пункт (c), см. срез 11 выше); остаётся: (c, продолжение) тот же bbox-сайзинг видимого слоя для `PushClip*`/`PushMask*` (срезы 12–13, тот же механизм — `screen_bbox_device_px`/`acquire_bbox_layer` — переиспользуется без изменений), viewport-cull для `PushMaskLayer` (SVG-`<mask>` content, сложнее — применяется к родительскому слою), а также `PushFilter`/`PushBlendMode` уже куллятся off-viewport (BUG-273 срез 1), но их видимый слой тоже full-framebuffer (срез 14).
4. Отложенные многокопийные image-кэши (см. диагностику 2026-07-07 в истории файла): femtovg `raw_images` deep-copy, `@WxH`-варианты, GIF все кадры, ~~font `bytes_store.cloned()`~~ (закрыто срезом 6 — `Arc<[u8]>`), canvas2d thread-local — актуально для image-heavy сайтов.

## Инструменты (остались в коде)

`LUMEN_MEM_REPORT=1` — периодический дамп размеров хранилищ в stderr (`about_to_wait`); `RenderBackend::debug_mem_report()`; `QuickJsRuntime::debug_memory_used()`; `DecodedImageCache/PrefetchCache::debug_stats()`.
