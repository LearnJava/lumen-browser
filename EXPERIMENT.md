# Ветка p1-exp-wgpu-only — экспериментальный полигон производительности

**СТАТУС: ЭКСПЕРИМЕНТАЛЬНАЯ ВЕТКА. В `main` НЕ ВЛИВАЕТСЯ. НИКОГДА.**
(решение пользователя 2026-07-08). Здесь разрешено тестировать любые технологии
и алгоритмы, ломать совместимость, снимать инварианты main-ветки. Цель —
ускорение движка **в 100 и 1000 раз** по конкретным метрикам. Удачные находки
переносятся в main **отдельными чистыми ветками**, не merge'ем этой.

Worktree: `.claude/worktrees/exp-wgpu-only`. Работать здесь, коммитить сюда.
`git push` — только по явной просьбе пользователя.

**Принцип эксперимента (директива пользователя 2026-07-09): пробовать ВСЕ
технологии и СМЕШИВАТЬ их, добавлять СВОИ решения — и искать скорость.**
Не ограничиваться одним образцом: приёмы из «Карты заимствований» ниже
(WebRender, GPUI, Slint, smithay, cosmic-text, …) можно и нужно комбинировать
в гибриды (например: SDF-эффекты GPUI внутри тайлов WebRender + damage-возраст
smithay + собственный patch_scroll_layer), а где чужого решения нет —
изобретать своё. Критерий отбора один: замер ДО/ПОСЛЕ тем же скриптом.
Неудачные комбинации откатывать и фиксировать в этом файле (как п.7),
удачные — оставлять и двигаться дальше.

---

## Что уже сделано (7+ коммитов, см. git log)

1. **OpenGL (femtovg/glutin) удалён полностью** (−4961 строк). Единственный
   рендер — wgpu `Renderer` (WGSL), `default = ["backend-wgpu"]`.
2. **BUG-274 диагностирован до корня**: wgpu/DX12 на Intel Iris Plus берёт
   ~2.3 мс CPU за закрытие КАЖДОГО render pass (вся стоимость в `drop(pass)`),
   не зависит от площади кадра (проверено `LUMEN_WINDOW=512x378` vs `2048x1512`),
   плюс разовый скачок памяти +500 МБ. Это фикс. оверхед пасса, не clear.
3. **Skip-identical-frame** в `Renderer::render`: тотальный хэш кадра +
   `content_generation` (бампается register_image/снапшотами/шрифтами/resize).
   Идентичный кадр не рисуется вовсе. Выкл: `LUMEN_NO_FRAME_SKIP=1`.
   Проверка: 5 с движения мыши над статикой = 31 skip / 2 рендера.
4. **patch_scroll_layer**: скролл overflow-контейнера правит DL in-place
   (PushScrollLayer + DrawScrollbar thumbs) вместо полной пересборки
   paint_ordered. 12×: 1.54 → 0.13 мс/тик. Эквивалентность пересборке
   закреплена 5 тестами `patch_scroll_layer_*` + бенч (ignored).
5. **Безаллокационный HashFmt** в hash_commands / diff_display_lists / TileGrid.
6. **Пофазная диагностика кадра** (см. env-переменные ниже).
7. Слияние соседних Draw-батчей уровня 0 — испробовано и **откачено**:
   в реальных планах соседних level-0 батчей не бывает (между ними composite).
8. **Ярус 1, срез 1 — кэш шрифтовых метрик + мемоизация резолва** (2026-07-09):
   `FaceMetrics` (owned: units_per_em, ascent/descent, `OwnedCmap`, advances)
   строится один раз при загрузке face; codepoint-cascade и advance больше
   не парсят шрифты. `Font::parse` остался только на промахе глиф-атласа и
   для variation axes — лениво через per-frame memo (`LazyParsedFaces`).
   Плюс мемоизация `resolve_face_id` (хэш families+weight+style → face_id,
   сброс в `set_font_provider`) — раньше КАЖДЫЙ DrawText каждого кадра гонял
   `to_lowercase` + `FontProvider::pick_face` (2 Vec-аллокации + матчинг).
   Замер (1000000-final.html, dev-release): CPU к t=5s **3218.8 → 2359–2547 мс
   (−21%)**; холодный кадр faces 201.6 → 172.9 мс. **Вскрытая правда: посылка
   очереди «Font::parse 2–4 мс тёплый» была неверна** — тёплый парс был <1 мс;
   тёплый остаток фазы faces (~1.4 мс) — это skip-frame `hash_display_list` +
   image-pre-pass, а холодные 172 мс — `fs::read`+WOFF-декод lazy-загрузки
   face-ов внутри первого кадра (новые пункты очереди).
9. **Ярус 0 — авто-проба бэкенда** (`crates/engine/paint/src/backend_probe.rs`):
   при старте для кандидатов Vulkan → GL → DX12 рисуется пробный кадр (clear
   в характерный цвет) в поверхность окна; два сигнала — texture readback
   (COPY_SRC + map) и **захват реальной презентации** через
   `PrintWindow(PW_CLIENTONLY|PW_RENDERFULLCONTENT)` — именно он ловит BUG-275
   (белое Vulkan-окно при «исправном» рендере). Кандидат принят, если
   презентация совпала с пробным цветом. Стоимость ~0.5 с на старте, окупается
   до t=5s. Выкл: `LUMEN_NO_BACKEND_PROBE=1`; `WGPU_BACKEND=...` главнее пробы.
   Замер (1000000-final.html, dev-release): idle-10s CPU 765.6 → **250 мс (3×)**,
   CPU к t=5s 4469 → 2219 мс, скачок памяти +185 МБ → нет (−28 МБ),
   private к t=15s 993 → 720 МБ. **Попутная находка: BUG-275 в этот вечер НЕ
   воспроизводится** — форс-Vulkan рисует корректно (транзиентный драйверный
   глюк); проба как раз и защищает от его возврата.

## Замеры бэкендов (idle-CPU за 10 с, graphic_tests/1000000-final.html, dev-release, Intel Iris Plus)

| Бэкенд | idle 10 с | Прогретый кадр | Картинка |
|---|---|---|---|
| femtovg/OpenGL (эталон main) | 375 мс | ед. мс | ок |
| wgpu **DX12** (текущий дефолт) | ~1500–2500 мс | 450–950 мс (wall) | ок |
| wgpu **Vulkan** | 203 мс | **7 мс** | **БЕЛОЕ ОКНО — BUG-275** |
| wgpu **GL** (`WGPU_BACKEND=gl`) | **156 мс — лучший** | 104–122 мс (wall, блокирующий GL) | ок |

Выбор бэкенда: **ярус-0 авто-проба** (Vulkan → GL → DX12, принимается первый,
чей пробный кадр реально виден в DWM-захвате). Статическая цепочка
DX12 → Vulkan → GL осталась резервом (`LUMEN_NO_BACKEND_PROBE=1` / все
кандидаты отклонены). Вопрос «GL дефолтом» снят: проба сама берёт Vulkan,
когда он презентует честно (сейчас — да), и GL, когда Vulkan белеет.

## Чужие решения — карта заимствований (обзор успешных Rust-проектов, 2026-07-09)

Обзор WebRender/Servo, Zed GPUI, egui, Vello, rerun, Bevy, Slint, alacritty,
wezterm, smithay/niri, iced, cosmic-text/swash/glyphon. Сгруппировано по нашим
проблемам; у каждого приёма — конкретные имена типов для поиска в исходниках.

### Проблема «~270 render pass'ов на кадр» (корень BUG-274) → 2–5 пассов

- **WebRender**: display list → **RenderTaskGraph**; топологическая сортировка
  (`RenderTaskGraphBuilder`, `render_on: PassId`) сливает все независимые
  offscreen-задачи в ОДИН пасс — число пассов ≈ глубине вложенности эффектов
  (типично 2–5), не числу эффектов. Offscreen-задачи размером с **bbox**
  (клип по viewport, кламп blur ≤300px), пакуются `GuillotineAllocator`-атласом
  в общие таргеты ~2048×2048 из **пула** (`return_render_target_to_pool`).
  Плюс opaque pass front-to-back с z-буфером + alpha pass back-to-front;
  цель «≤100 draw calls на кадр».
- **GPUI (Zed)**: вообще без offscreen-слоёв на эффект — фиксированные
  примитивы (shadows → quads → paths → glyphs → sprites → images), каждый тип =
  один instanced draw call. Скругления/рамки — SDF в шейдере квада; box-shadow —
  замкнутая формула Эвана Уоллеса (erf-аппроксимация гаусса) прямо в шейдере,
  никаких blur-пассов; один общий MSAA-таргет на все пути кадра. Показательно:
  Windows-бэкенд Zed — **DX11, не DX12** («сложность не окупается») — наш
  wgpu-GL-фолбэк идейно то же самое.
- **WebRender, opacity**: простая `opacity` — «opacity collapse» в модуляцию
  alpha вершинных данных, слоя нет вовсе; маски — только сегменты с углами
  (`BrushSegment`) в общий alpha-атлас с кэшем между кадрами (`RenderTaskCache`);
  mix-blend читает backdrop из тайла кэша, а не копирует кадр.
- **re_renderer (rerun)**: фиксированные draw phases, фаза ≈ один пасс,
  drawable ≈ один draw call — число пассов константно и не зависит от сцены.

### Проблема «скролл/GIF перерисовывают весь кадр» (ярус 1)

- **WebRender picture caching**: контент растеризуется в крупные тайлы
  (~2048×512) в координатах **scroll root** (`TileCacheInstance`) — скролл =
  сдвиг offset'а при композиции, рисуются только въехавшие тайлы. Инвалидация:
  per-tile списки зависимостей (примитивы/клипы/image keys/opacity bindings) +
  интернинг примитивов → сравнение по id кадр-к-кадру → dirty rect + scissor
  внутри тайла. GIF в углу инвалидирует один тайл. Итог Mozilla: скролл
  16.4 Вт → 9.4 Вт.
- **smithay `OutputDamageTracker`** — каноничный damage-алгоритм: элементы с
  commit-счётчиками; damage = изменившиеся + появившиеся/сдвинувшиеся (старый
  И новый rect); **накопление damage по возрасту буфера** (при double/triple
  buffering перерисовывается union damage последних age-1 кадров); при
  фрагментации регион упрощается до extents.
- **alacritty**: построчный damage (`LineDamageBounds`), damage двух кадров,
  старый курсор/выделение добавляются в damage. **iced-урок**: их GPU-путь до
  сих пор без damage, но софтверный `iced_tiny_skia` диффит примитивы — диффинг
  display-list'а работает без реактивной системы свойств (наш
  `diff_display_lists` уже есть — его надо ПОДКЛЮЧИТЬ к рендеру).
- **Slint**: `PropertyTracker` → `DirtyRegion` (до 3 прямоугольников, старая +
  новая геометрия), история damage на 3 буфера.

### Проблема «Font::parse всех face каждый кадр» (ярус 1)

- **cosmic-text `FontSystem`** — эталон: fontdb парсит метаданные ОДИН раз при
  загрузке, данные в `Arc`; кэш `Font`-объектов по font_id (внутри распарсенный
  Face), кэши font-matching и codepoint→face (fallback). Поверх —
  `ShapeRunCache` (кэш шейпинга целых run'ов, Bevy получил 82→90–100 fps) и
  `SwashCache` (растр глифа по ключу face_id+glyph+size+subpixel).
- **GPUI/egui/glyphon**: глиф-атлас персистентный, alpha-only, тонирование
  цветом в шейдере; glyphon разделяет `prepare()` (CPU) и `render()` (один
  instanced draw в чужом пассе — middleware-паттерн wgpu).

### Проблема «idle ≠ 0» (BUG-271/274)

- **alacritty / niri / Slint / egui reactive mode** — общий принцип: нет
  таймера кадров вообще; redraw только от источников изменений (ввод, сеть,
  анимации, GIF-таймер); статичная страница = ноль wakeup'ов. Continuous-режим
  существует только как debug-флаг. Наш skip-identical-frame — полумера:
  кадр всё ещё СТРОИТСЯ и хэшируется; цель — не строить.

### Рендер вне UI-потока (ярус 2)

- **WebRender**: 3 потока — scene builder (тяжёлый CPU: picture/spatial/clip
  tree, интернинг) / render backend (culling, task graph, батчи) / renderer
  (единственный трогает GPU). Скролл и анимируемые свойства — property
  bindings в render backend, минуя пересборку сцены.
- **Bevy pipelined rendering**: фазы Extract → Prepare → Queue → Render; Extract —
  единственная точка синхронизации (копирование в render-мир), render-поток
  рисует кадр N, пока main считает N+1. **GPUI**: пул instance-буферов с triple
  buffering и асинхронным возвратом — нет CPU↔GPU stall'ов.

### Ярус 3 (Vello) — статус

Vello classic: клипы/слои/блэнды — команды PTCL внутри compute-конвейера
(вложенные эффекты не порождают пассов), но на конец 2025 — **alpha**:
worst-case GPU-память, conflation-артефакты, фильтры не доделаны. Зреет ветка
sparse strips: `vello_cpu` (быстрейший CPU-рендерер в Rust) и `vello_hybrid`
(CPU-геометрия + лёгкий GPU-финал, до WebGL2). Servo взял Vello для canvas.
Вердикт: **следить, не брать**; наш порядок ярусов подтверждается.

### Порядок внедрения (как шла Mozilla)

1. Граф задач + bbox-слои в атласе из пула → схлопывает 270 пассов;
2. тайловый кэш на scroll root → скролл почти бесплатен;
3. per-tile/damage инвалидация → GIF стоит свой прямоугольник;
4. вынос рендера в поток (extract-паттерн Bevy) — последним, когда кадр уже дёшев.

## Очередь работ (по приоритету, цель 100–1000×)

1. ~~Ярус 0 — авто-проба бэкенда~~ **СДЕЛАНО 2026-07-08** (см. «Что уже
   сделано» п.8): idle-CPU 3×, память −273 МБ, BUG-275 обезврежен пробой.
2. **Ярус 1 — не рисовать лишнее** (главный рычаг; приёмы — см. «Карта
   заимствований» выше):
   - ~~кэш `parsed_faces`~~ **СДЕЛАНО 2026-07-09** (см. «Что уже сделано»
     п.8: FaceMetrics + LazyParsedFaces + resolve-memo, CPU к t=5s −21%).
     Открытые хвосты этого направления:
     - лениво загружаемые face-ы читаются с диска (`fs::read` + WOFF-декод)
       ВНУТРИ первого кадра — 172 мс фазы faces. Вынести загрузку с
       render-пути (префетч после layout / фоновый поток);
     - тёплый остаток фазы faces ~1.4 мс = `hash_display_list` skip-frame
       хэша (Debug-fmt по 1062 командам) + image-pre-pass. Кандидат:
       инкрементальный/кэшируемый хэш DL вместо полного пересчёта;
   - dirty-rect до конца: `TileGrid`/`diff_display_lists` пишутся, но не
     читаются рендером — подключить scissor-ограниченную перерисовку
     изменившегося региона (GIF перерисовывает свои 200×150, не весь кадр);
     union damage по возрасту буфера — как smithay `OutputDamageTracker`;
   - **пасс-схлопывание**: offscreen-слои эффектов по bbox (не full-frame),
     независимые — в общий атлас-таргет одним пассом (WebRender
     RenderTaskGraph); простая opacity — модуляция alpha без слоя (opacity
     collapse); скругления/тени — SDF/формула Уоллеса в шейдере (GPUI).
     Это добивает остаток BUG-274 на любом бэкенде;
   - скролл-композитор страницы: тайлы в координатах scroll root, скролл =
     сдвиг offset'а, рисуются только въехавшие тайлы (WebRender picture
     caching); минимум — persistent-текстура с запасом + сдвиг матрицей.
3. **Ярус 2 — параллелизм**: рендер вон из UI-потока (скелет
   `ThreadedCompositor` есть; паттерн — Bevy Extract: UI строит только
   display list, render-поток владеет GPU и рисует кадр N, пока UI считает
   N+1; максимум пользы на Vulkan/DX12 — GL сериализуется локами);
   параллельный style/layout (rayon, Servo-style).
4. **Ярус 3 — Vello** (compute-растеризатор поверх wgpu; заглушка
   `vello_backend.rs` есть) — только после ярусов 0–2 и по замерам.
   Статус 2025: alpha, «следить, не брать»; смотреть также vello_hybrid.
5. Профиль боевой сборки: release + thin-LTO + PGO + mimalloc (10–20% всюду).
6. BUG-275: обновить драйвер Intel / проверить на другой машине.

## Диагностические env-переменные (добавлены в этой ветке)

| Переменная | Действие |
|---|---|
| `LUMEN_FRAME_LOG=1` | адаптер + параметры поверхности + шелл-лог `[frame]` |
| `LUMEN_FRAME_LOG=2` | + фазы кадра (faces/collect/prep/acquire/encode/submit), разбивка encode по RenderPlanItem, разбивка Draw-пасса, счётчик текстур |
| `WGPU_BACKEND=vulkan\|dx12\|gl` | явный выбор HAL-бэкенда wgpu |
| `LUMEN_NO_FRAME_SKIP=1` | выключить skip-identical-frame |
| `LUMEN_NO_BACKEND_PROBE=1` | выключить ярус-0 авто-пробу (статическая цепочка DX12 → Vulkan → GL) |
| `LUMEN_PRESENT=mailbox\|immediate` | present mode (дефолт Fifo) |
| `LUMEN_WINDOW=WxH` | размер окна (эксперименты с площадью кадра) |

## Методика замеров

- Сборка: `cargo build --profile dev-release -p lumen-shell` (НЕ `--release` —
  запрещено правилами репо для тестов, dev-release 2–3× быстрее собирается).
- Idle-CPU: `powershell -ExecutionPolicy Bypass -File scripts/exp/measure_idle.ps1
  -FrameLog 2` — CPU/WS/private на t=5s и t=15s, stderr в `.tmp/idle_stderr.log`.
- Скриншот окна: `scripts/exp/printwindow.ps1 -Backend <hal> -Out <png>`
  (PrintWindow PW_RENDERFULLCONTENT). gdigrab полного стола — второй способ.
- Бенч скролл-патча: `cargo test -p lumen-paint --release patch_scroll_layer_bench
  -- --ignored --nocapture`.

## Грабли (уже наступали — не повторять)

- **«Белая страница + таб-бар» = страница НЕ ЗАГРУЗИЛАСЬ** (os error 2 при
  относительном пути из PowerShell), а не баг рендера. Проверяй stderr на
  «Ошибка загрузки» и заголовок окна.
- **Vulkan-окно белое целиком** (и таб-бар тоже) — это BUG-275, рендер при
  этом «исправен» по всем логам. Оба способа захвата показывают белое.
- graphic_tests/run.py калиброван под femtovg-пиксели — на этой ветке его
  пороги не показательны; визуальную проверку делать скриншотом + глазами
  (Read PNG) против DX12-эталона.
- `LUMEN_FRAME_LOG` читается один раз за процесс (OnceLock) — менять уровень
  между кадрами нельзя.
- Правки делать по пути worktree (`.claude/worktrees/exp-wgpu-only/...`),
  не по main-root — иначе уедут в чужое дерево.

## Протокол следующей сессии

1. `git branch` → убедиться, что `p1-exp-wgpu-only` существует; работать в
   worktree `.claude/worktrees/exp-wgpu-only`.
2. Прочитать этот файл + `bugs/BUG-274-OPEN.md` + `bugs/BUG-275-OPEN.md`.
3. Взять верхний пункт из «Очереди работ», перед оптимизацией — замер ДО,
   после — замер ПОСЛЕ тем же скриптом, цифры в коммит.
4. Помнить: в main НЕ вливать. Удачные находки — списком в этот файл
   (раздел «Кандидаты на перенос в main» — завести при первом кандидате).
