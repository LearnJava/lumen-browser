# Как работать над скролл-перфом на Linux

Точка входа для новой Claude-сессии на этой Linux-машине. Читать ПЕРВЫМ, до
любого кода. Дополняет [`linux-wgpu-validation.md`](linux-wgpu-validation.md)
(там — история валидации Vulkan-пути и матрица замеров) практическим «как».

> **СТАТУС 2026-07-13 (важно, читать первым):** ветка `p1-wgpu-cross-platform`
> **устарела** — весь её трек (bbox-scissor + strip/blit-композитор) и ещё
> несколько приёмов уже в `main` через параллельный трек **Ph-wgpu-default**,
> причём компоновщик **включён по умолчанию** (kill-switch'и `LUMEN_NO_*`, см.
> ниже). WIP-баг bbox×Band из ветки в main отсутствует. Актуальный трек —
> ветка `p1-linux-scroll-perf` (влита в main, worktree удалён). Результаты
> валидации main-компоновщика на этой машине — в разделе «Результаты — main-трек».
>
> **Новая сессия начинает отсюда → раздел [«СЛЕДУЮЩАЯ СЕССИЯ — что делать»](#следующая-сессия--что-делать-подробно)**
> (пошаговый план: рекомендованный срез scroll-copy + запасные + протокол замера).

## TL;DR

- **Ветка/worktree (актуально):** `p1-linux-scroll-perf`, worktree
  `.claude/worktrees/linux-scroll-perf`, собран **от main**. Прежний
  `p1-wgpu-cross-platform` / `.claude/worktrees/wgpu-validation` — устарел
  (см. статус выше). Windows-only `p1-exp-wgpu-only` в `main` не вливается
  никогда; её `EXPERIMENT.md` — журнал 25 срезов, справочник приёмов, но её
  PowerShell-харнесс на Linux не работает.
- **Работать из каталога worktree**, коммитить в `p1-linux-scroll-perf`,
  `git push` — только по явной просьбе пользователя.
- **Железное правило (выведено кровью на exp-ветке):** ни строчки оптимизации
  без замера ДО тем же скриптом, которым будет замер ПОСЛЕ. Шесть посылок подряд
  на exp-ветке оказались ложными — все потому, что оптимизацию начинали до замера.

## Машина (зафиксировано 2026-07-13)

| | |
|---|---|
| ОС | CachyOS (Arch), ядро 7.1.x |
| Дисплей-сервер | **Wayland** (`WAYLAND_DISPLAY=wayland-0`); X11 тоже поднят (`DISPLAY=:0`) |
| GPU | Intel HD Graphics 530 (SKL GT2), драйвер **Mesa Vulkan** |
| Rust | stable (уже 1.97+ на Linux — новые clippy-линты бьют по старому коду) |
| sccache | **НЕ установлен** |

## Обход граблей окружения

- **sccache.** `.cargo/config.toml` жёстко задаёт `rustc-wrapper = "sccache"`, а
  бинарника нет → любая cargo-команда падает. Обходить пер-командно:
  ```bash
  RUSTC_WRAPPER="" cargo build -p lumen-shell --release
  ```
  (не править config.toml — он общий с Windows, где sccache есть).
- **Профиль сборки для перфа — `--release`** (не dev-release: на этой машине
  release-бинарник уже лежит в `target/release/lumen`). Правила репо запрещают
  `--release` для graphic_tests, но здесь это перф-замер, а не тест-гейт.
- **Выбор бэкенда (main):** дефолт — уже **wgpu** (probe Vulkan→GL→DX12, ADR-017),
  так что перф-путь = дефолт. `LUMEN_BACKEND=wgpu` можно оставлять для явности,
  `LUMEN_BACKEND=femtovg` — детерминированный femtovg. femtovg vsync-bound —
  его медианы это пол, не цена.
- **Сборка main НЕ КОМПИЛИТСЯ на Linux из коробки** (проверено 2026-07-13):
  `crates/shell/src/platform/display_color_profile.rs` реэкспортил
  `NullDisplayColorProfile` без пути и без `::new()` → E0432. Фикс (локальный
  не-Windows impl `PlatformDisplayColorProfile`) в ветке `p1-linux-scroll-perf`.
  Плюс на Rust 1.97 висят pre-existing clippy-линты (`question_mark`,
  `byte_char_slices`) в dom/image/layout — это дрейф main, не наш код (P5).
- **ГРАБЛЯ: `pkill -f "target/release/lumen"` убивает СВОЙ ЖЕ shell** — строка
  запуска Bash-инструмента содержит этот путь, `pkill -f` матчит её и шлёт
  SIGTERM самому себе (выход 144, пустой лог, python не при чём). Убивать
  процесс только точным именем: **`pkill -x lumen`**.
- **ГРАБЛЯ: `1000000-final.html` высотой ~1e6 px** — edge-to-edge проход MCP
  никогда не завершается (`passes 0` навсегда), внешний `timeout` рубит python
  до печати. Ограничивать внутренним `--timeout 12..15` (тогда цикл выходит по
  дедлайну и печатает перцентили) — НЕ внешним `timeout`.

## Замеры

**Основной инструмент — [`scripts/bench_scroll.py`](../../scripts/bench_scroll.py)**
(Linux-контрчасть PowerShell `run_warm_frame_bench.ps1` + `proc_stats.ps1` с
exp-ветки). Поднимает `target/release/lumen`, скроллит страницу
край-в-край-и-обратно, семплит CPU/PSS из `/proc`, печатает медиану/p95 кадра +
скорость скролла + CPU% + пик PSS.

```bash
cd .claude/worktrees/linux-scroll-perf
python3 scripts/bench_scroll.py --page samples/bench-static-scroll.html \
    --backend wgpu --step 60 --runs 3
```

- **`--driver mcp` (дефолт) — единственный надёжный путь на Wayland.** Скроллит
  через live-окно по MCP (`--mcp-live-port`), спейсит подачу по `[frame]`-логу —
  это интерактивный путь ввода.
- **`--driver bench` (LUMEN_BENCH) на Wayland ЗАВИСАЕТ** — цикл about_to_wait
  голодает swapchain acquire на KWin/Wayland (Timeout каждые ~4 кадра =
  глубина swapchain), а перекрытое окно вообще не получает frame callbacks.
  `LUMEN_BENCH` оставлен корректным для Windows и `--dump`-режимов.
- **Стенд проверяет сам себя:** в строке отчёта есть `rendered N skipped M`; при
  `scroll ... rendered 0` печатает `INVALID` (страница не скроллится / окно
  перекрыто). Нет этой строки — стенду не верить.
- **Фон двигает медиану на десятки процентов** (15→11 мс на одном бинарнике):
  замер ДО и ПОСЛЕ снимать в один заход, тем же скриптом, 3 прогона.

**Реальный GPU-скриншот** (для пиксельной A/B-сверки): MCP-ресурс `screenshot`
рендерит через `render_to_image_cpu` — это НЕ вывод GPU. Реальные пиксели
wgpu-окна снимает KDE `spectacle` по активному окну (KWin-скрипт активирует окно
Lumen). Windows-инструменты (PrintWindow, ffmpeg gdigrab) на Linux не работают.

**Эталон Chromium/Chrome:** есть Chromium, драйвится по CDP (rAF-цикл scrollBy,
та же страница/шаг, изолированный профиль). Числа индикативны (Chromium
smooth-scroll'ит колесо — работа на событие не идентична).

## Бенч-страницы

- `samples/bench-static-scroll.html` — 40 секций blur/градиенты/тени, ноль анимаций
  (чистый скролл-кейс). **Сейчас заклинивает под wgpu — BUG-276** (SurfaceLost×21).
- `samples/bench-anim-scroll.html` — 40 секций scroll-driven анимаций (transform +
  background), детерминированы позицией скролла.
- `graphic_tests/1000000-final.html` — kitchen-sink (2013 узлов, 12 картинок).

## Что уже известно про скролл на Linux (linux-wgpu-validation.md §матрица)

- wgpu бьёт femtovg 3.6× на стресс-странице, паритет на лёгких (<1 мс).
- Против Chromium main-рендерер **проигрывает войну тяжёлого скролла: 7.5 fps
  против 60** — потому что каждый кадр перерисовывает весь display list, а
  Chromium композитит пред-растеризованные слои. При этом Lumen ест 5× меньше CPU
  и меньше RAM.
- **Разрыв закрывают приёмы exp-ветки, НЕ портированные в этот трек:**
  bbox-scissor фильтр-пассов (стресс 3.2–4×, пиксели бит-в-бит) → viewport-cull
  невидимых слоёв (ещё 1.27×) → strip+blit скролл-композитор (статика 6–8×) →
  static/animated split. Порядок внедрения (как шла Mozilla): сначала bbox-слои,
  потом тайловый кэш скролла, потом damage, вынос в поток — последним.
- **BUG-276** (`bugs/BUG-276-OPEN.md`) — swapchain wedge на blur-странице, его
  чинит тот же bbox-путь.

## Порядок работы на этой сессии

1. Замер ДО: `bench_scroll.py` на 1000000-final + bench-anim-scroll, wgpu, 3 прогона.
2. Портировать первый приём (bbox-scissor — самый безопасный, пиксели идентичны,
   kill-switch `LUMEN_NO_BBOX_SCISSOR`). Механику брать из `EXPERIMENT.md` п.16
   ветки `p1-exp-wgpu-only` (`git show origin/p1-exp-wgpu-only:EXPERIMENT.md`).
3. Замер ПОСЛЕ тем же скриптом, в тот же заход. Пиксельная A/B через kill-switch.
4. Зафиксировать в этом файле числа ДО/ПОСЛЕ и в коммите.

## Результаты — main-трек (2026-07-13)

Валидация компоновщика `main` (Ph-wgpu-default, включён по умолчанию) на этой
машине. Бинарник — release от `main` (+ фикс сборки display_color_profile).
Инструмент — `bench_scroll.py --driver mcp`, wgpu/Vulkan/Intel HD 530,
`LUMEN_PRESENT=immediate`, 3 прогона, `--timeout 12`.

**Baseline (компоновщик по умолчанию, всё включено):**

| Страница | медиана (mm) | p95 | CPU | PSS |
|---|---|---|---|---|
| `1000000-final.html` step 60 | **4.68 мс** (~200 fps) | ~10 мс | 40 % | 274 МБ |
| `bench-static-scroll.html` step 120 | ~5 мс (разброс 4–13) | 11–54 мс | 16–26 % | 218 МБ |
| `bench-anim-scroll.html` step 120 | **2.18 мс** | ~22 мс | 48 % | 236 МБ |

Медиана быстрая (кадры = blit-HIT полосы), но **хвост (p95) и разброс между
прогонами держит band-MISS** (полная переросфинкция полосы ~1024×1800 при выходе
скролла за пределы полосы = вьюпорт + ≤768 px запаса → MISS каждые ~768 px
скролла). Ноль SurfaceLost/клинов — BUG-276 действительно закрыт в main.

**A/B kill-switch на `1000000-final` (один бинарник, back-to-back, 3 прогона):**

| Конфиг | медиана | mean | p95 | скорость | CPU |
|---|---|---|---|---|---|
| default (всё вкл) | **4.68 мс** | 5.4 | ~10 | 1413 px/s | 40 % |
| `LUMEN_NO_SCROLL_COMPOSITOR=1` | 7.05 мс | 6.4 | ~11 | 1415 | 43 % |
| `LUMEN_NO_ANIM_SPLIT=1` | 4.95 мс | 7.7 | ~22 | 1382 | 44 % |
| `LUMEN_NO_BBOX_SCISSOR=1` | 4.60 мс | 28.4 | **259** | **815** | 26 % |

Вклад приёмов на слабой iGPU:
- **bbox-scissor — фундамент.** Медиану не трогает (blit-HIT), но без него
  band-MISS-кадры делают 40 полноэкранных blur-пассов → **p95 259 мс, скорость
  вдвое, GPU-bound столл (CPU падает до 26 %)**. Это тот самый 11× выигрыш.
- **scroll-компоновщик — ~34 % по медиане** (7.05→4.68). Blit НЕ съедает выигрыш
  на этой iGPU — снимает открытый вопрос старой ветки (боялись, что съест).
- **anim-split — глушит хвост** (mean 7.7→5.4, p95 22→10) на анимируемом
  контенте; медиану почти не трогает.

**Эталон Chromium (rAF, тот же page/step, изолированный профиль):**

| Движок | медиана | p95 | CPU | RAM |
|---|---|---|---|---|
| **Lumen** (main, всё вкл) | 4.68 мс | ~10 мс | **40 %** | **274 МБ** |
| Chromium | 3.1 мс | 5.4 мс | 338 % | 515 МБ |

Компоновщик закрыл разрыв кадра по времени с **~18×** (было 133 мс vs ~7 мс,
довкомпоновщиковый замер) до **~1.5×**, при этом Lumen ест **~8× меньше CPU** и
**~1.9× меньше RAM**. Оба движка глубоко под бюджетом 16.7 мс/60 fps → скроллят
плавно.

### Срез — направленный сдвиг полосы (2026-07-13, СДЕЛАНО, default-on)

Профилирование MISS/HIT (`scripts/miss_probe.py`, LUMEN_FRAME_LOG=2, коррелирует
`[frame] total` с `page-compose MISS/HIT`) показало: MISS **GPU-fill-bound**
(CPU-encode band-рендера ~0.5 мс, но кадр 50–64 мс — GPU растеризует всю полосу
1890 px с блюрами) и на реалистичной статике (`bench-static-scroll` step 120)
**MISS = 20 % кадров и 39 % времени скролла**, отдельные кадры до 64 мс (4×
бюджет 16.7 мс → видимый джанк). HIT (blit) — 80 % кадров, но дёшев.

Полный scroll-copy (копировать перекрытие + растеризовать только новую полосу)
режет **цену** MISS, но это глубокая хирургия render_impl (slice-scissor через
всю clear/scissor-машину — та же зона, где прошлая сессия словила bbox×Band-баг)
и трудно доказать пиксель-в-пиксель. Взят более дешёвый и безопасный приём,
бьющий в ту же цель через **частоту** MISS:

**Направленный сдвиг.** Полный запас полосы = `2*margin`. Симметрия ставит
вьюпорт по центру → MISS после ~½ запаса в любую сторону. Скролл почти всегда
непрерывен, поэтому при промахе кладём **80 % запаса ПО ходу** движения
(направление — из старой полосы: `scroll_y < band_top` ⇒ вверх, иначе вниз),
вьюпорт садится у «хвостового» края. Меняет только ПОЛОЖЕНИЕ полосы, не пиксели
(fits-check гарантирует вьюпорт ⊆ полоса) → пиксельно идентично. Kill-switch
`LUMEN_NO_BAND_BIAS=1`. Правка — только `try_page_compose` (расчёт `band_top_css`).

**Замер (`scripts/miss_probe.py`, bench-static-scroll step 120, self-A/B на одном
release-бинарнике, OFF=`LUMEN_NO_BAND_BIAS=1` / ON=default):**

| Паттерн | | MISS (кадров) | wall-время | MISS-доля времени |
|---|---|---|---|---|
| одно направление | OFF → ON | 50 → **32** (−36 %) | 1669 → **1439 мс** (−14 %) | 39 % → 28 % |
| 1 разворот (updown) | OFF → ON | 50 → **32** | 2074 → **1740 мс** (−16 %) | 46 % → 35 % |
| 12 разворотов (zigzag) | OFF → ON | 43 → **31** | 2031 → **1742 мс** (−14 %) | 46 % → 36 % |

Цена одного MISS не изменилась (та же полная полоса, просто реже), HIT не тронут.
Опасение «разворот съест выигрыш» не подтвердилось: даже разворот каждые 20
скроллов оставляет −14 % (перецентровка при промахе переориентирует запас в новую
сторону). На анимированной `1000000-final` (MISS всего ~5 %) — без регресса.

## СЛЕДУЮЩАЯ СЕССИЯ — что делать (подробно)

Состояние: медиана уже отличная (≈4.7 мс, 200 fps, 8× экономия CPU vs Chromium).
Единственный оставшийся зазор — **хвост (p95) на band-MISS-кадрах**. Направленный
сдвиг (срез выше) снял ~⅓ MISS через их ЧАСТОТУ. Следующий уровень — резать
**ЦЕНУ** одного MISS. Ниже — рекомендованная задача пошагово, затем запасные.

### Шаг 0 — старт сессии (обязательно, ~5 мин)

1. `git -C <repo> fetch origin && git -C <repo> pull --ff-only` (main мог уйти
   вперёд — параллельные сессии).
2. Новый worktree ОТ MAIN: `git worktree add .claude/worktrees/scroll-copy -b p1-scroll-copy main`.
   Работать ИЗ каталога worktree (иначе cargo соберёт primary/main, не ветку).
3. Прочитать этот файл целиком + раздел «Результаты — main-трек» (числа-базлайн)
   + `subsystems/paint.md` пункт «wgpu scroll-compositor directional band-bias»
   (там разобран путь компоновщика и где что лежит).
4. Собрать базлайн-бинарник: `RUSTC_WRAPPER="" cargo build -p lumen-shell --release`
   (обход sccache — его на машине нет). Прогнать `scripts/miss_probe.py
   samples/bench-static-scroll.html 120 250` — ЗАПИСАТЬ числа ДО (MISS n, median,
   p95, max; MISS-доля времени). Это точка отсчёта для A/B.

Грабли окружения — см. раздел «Обход граблей окружения» выше (главное: `pkill -x
lumen`, НЕ `-f "…lumen"`; внутренний `--timeout`, не внешний; wgpu — дефолт).

### Шаг 1 (рекомендовано) — scroll-copy: резать ЦЕНУ band-MISS

**Идея.** Сейчас при промахе (`try_page_compose`, ветка `if !fits`) полоса
перерисовывается ЦЕЛИКОМ (`render_impl(Band)` рисует всю ~1890-px полосу — на
Intel HD 530 это 50–64 мс GPU-fill, доказано `LUMEN_FRAME_LOG=2`: CPU-encode
~0.5 мс, остальное — GPU-растеризация блюров). Но при непрерывном скролле старая
и новая полосы ПЕРЕКРЫВАЮТСЯ на ~⅔. Приём: скопировать перекрытие из старой
текстуры в новую (сдвиг) и растеризовать ТОЛЬКО вновь открывшуюся полосу под
scissor'ом → цена MISS падает ~3× (только ~⅓ высоты рисуется заново).

**Готовый эталон в этом же крейте — femtovg-путь уже это умеет.** НЕ изобретать:
- `crates/engine/paint/src/scroll_cache.rs` — `enum ScrollFramePlan` (вариант
  `BlitAndExpose { origin, size, retained: Rect, prev_origin, expose: [Option<Rect>;4] }`),
  `fn plan(...)` (решает Blit/BlitAndExpose/Repaint по хэшу+перекрытию),
  `fn band_blit_placement(...)` (куда класть blit), тайлинг-инвариант
  `retained + expose == band`. Это ЧИСТАЯ логика (12+ тестов) — переиспользовать её.
- `crates/engine/paint/src/backends/femtovg_backend.rs` — `blit_retained_band`
  (FBO→FBO копия перекрытия), `run_content_pass(content, band_src, scissor, …)`
  (один проход с scissor'ом в scroll-координатах — рисует только strip).
  M3.2.1b-2b в `subsystems/paint.md` — подробный разбор.

**Что менять (wgpu-путь), по шагам:**
1. `PageBandCache` (`renderer.rs:~1303`) — добавить ВТОРУЮ текстуру полосы
   (ping-pong; держать вне общего пула, чтобы repaint пинг-понгал буфер). Сейчас
   поле одно (`_texture`/`view`/`depth_t`/`depth_v`).
2. `try_page_compose` (`renderer.rs:~4909`), ветка `if !fits` при
   `content_stable == true` И непустом перекрытии старой/новой полосы:
   a. Посчитать перекрытие в device-текселях (texel-align: `band_top` уже
      floor'ится до целого CSS px; ×dpr → целые строки). Опора —
      `ScrollCache::plan`/`band_blit_placement` из scroll_cache.rs.
   b. `encoder.copy_texture_to_texture(старая → новая)` для региона перекрытия
      со сдвигом.
   c. Растеризовать `static_content` в новую текстуру через `render_impl(Band)`,
      НО: scissor только на строки новой полосы + `LoadOp::Load` уровня-0 цели
      (сохранить скопированное перекрытие; сейчас там Clear — он затрёт копию),
      очистка только slice-региона (квад bg-цвета под scissor'ом).
   d. Свап `page_band` на новую текстуру.
   e. Guard: если key сменился ИЛИ перекрытия нет (прыжок/резайз) → нынешний
      полный re-raster (всегда корректно).
3. Kill-switch `LUMEN_NO_SCROLL_COPY=1` рядом с `band_bias_disabled()` etc.
   (`renderer.rs:~1936`, паттерн `OnceLock`). **Первый заход — ставь дефолт OFF**,
   включишь после пиксель-A/B.

**Где рискует (тут прошлая сессия ловила bbox×Band-баг):** slice-scissor должен
пройти через всю clear/scissor-машину `render_impl` (см. `RenderPassMode::Band`
на `renderer.rs:~7503`, сборку пасса уровня-0 с `LoadOpChoice` ~7627). Каждый
per-op scissor (`sync_scissor_to_stack`) надо пересечь с slice-регионом; LoadOp
уровня-0 в Band должен стать `Load`, а очистка — только slice. Легко получить
«тесный неверный scissor в offscreen» → пропавшие блюр-боксы. **ОБЯЗАТЕЛЬНА
пиксельная A/B** (kill-switch ON vs OFF) через `spectacle` по активному окну
(см. «Реальный GPU-скриншот» выше) на `samples/bench-static-scroll.html` (блюры)
и `graphic_tests/30-css-filter.html` (все фильтры) — картинки должны совпасть
бит-в-бит.

**Приёмка среза:**
- `miss_probe.py` bench-static-scroll: MISS median 6→~2 мс, max 64→~20 мс, при
  том же MISS n (частоту режет band-bias, цену — этот срез). Комбинированно с
  band-bias хвост p95 упадёт заметно.
- Пиксель-A/B ON vs OFF — идентично (иначе НЕ включать по умолчанию).
- `cargo clippy -p lumen-shell --release -- -D warnings` чист (на Linux он
  ЗЕЛЁНЫЙ после этой сессии — не сломать); `cargo test -p lumen-paint --lib`.
- Доксинк: этот файл (числа ДО/ПОСЛЕ) + `subsystems/paint.md` (пункт про
  scroll-copy) + BUGS.md если что чинит.

### Запасные задачи (если scroll-copy окажется слишком рискованным)

1. **Тайловый кэш полос** (координаты scroll-root, «финальная форма» урока
   п.15/19): держать несколько закэшированных полос → возврат в уже отрисованный
   регион = HIT, не MISS. Больше помогает реальному туда-сюда-скроллу, чем
   одно-направленному bench-у. Риск ниже (не трогает render_impl clear/scissor),
   выигрыш на bench-профиле меньше.
2. **Хвост `bench-anim-scroll`** (p95 ≈22 мс при median 2.2 мс): анимируемые
   сегменты-оверлеи перерисовываются каждый кадр. Профилировать `miss_probe.py`
   + `LUMEN_FRAME_LOG=2` (разбивка по типам команд) — где именно 22 мс: overlay
   fill, пересбор вершин, или периодический band-MISS. Затем точечный приём.

### Железное правило замера (не нарушать)

Любой срез мерить `scripts/miss_probe.py` (атрибуция MISS/HIT) И
`scripts/bench_scroll.py` (median/p95/CPU/PSS) **back-to-back на одном
бинарнике**: default vs kill-switch, 3 прогона, сравнивая **p95/mean/MISS-долю**,
не только медиану. Фон двигает медиану на десятки % — замер ДО и ПОСЛЕ в один
заход. Ни строчки оптимизации до замера ДО (шесть ложных посылок на exp-ветке —
все из-за оптимизации раньше замера).

## Результаты — старый branch-трек `p1-wgpu-cross-platform` (архив, устарел)

> Ниже — журнал устаревшей ветки. Оба её среза уже в main (см. main-трек выше).
> Оставлено как справочник механики приёмов.

### Срез 1 — bbox-scissor фильтр-пассов (2026-07-13, СДЕЛАНО)

Портирован приём п.16 exp-ветки в `renderer.rs` кросс-платформенной ветки:
`LevelBounds` (bbox нарисованного контента на offscreen-уровень; объединение
вершин draw-ops + дочерних композитов) → на `PopFilter` считается
`set_scissor_rect` с блюр-паддингом `min(ceil(3σ),32)+2` текселя → 3 фильтр-пасса
(blur H/V + composite) красят только bbox элемента, а не весь экран. Пустой /
за-экранный слой пропускается целиком. Kill-switch `LUMEN_NO_BBOX_SCISSOR=1`.
Mask/backdrop-композиты помечают родителя `Unbounded` (безопасный фолбэк на
полноэкранные пассы). Правки: декларации + 9 Push + 6 Pop сайтов + 3 render-scissor.

**Замер (self-A/B на одном dev-release бинарнике, kill-switch ON=до / OFF=после,
2 прогона, wgpu/Vulkan/Intel HD 530, LUMEN_PRESENT=immediate):**

| Страница | До (scissor OFF) | После (scissor ON) | Ускорение |
|---|---|---|---|
| `graphic_tests/1000000-final.html` step 60 | 133.2 мс, 433 px/s, PSS 343 МБ | **11.9 мс**, 1346 px/s, PSS 294 МБ | **11.2×** |
| `samples/bench-static-scroll.html` step 200 (**BUG-276**) | клин ~0.5 fps, SurfaceLost×21 | **17.0 мс (~59 fps)**, 4567 px/s, 0 SurfaceLost | клин устранён |

Эффект больше, чем на Windows (там 4× — п.16 exp): слабая Intel HD 530 сильнее
упирается в fillrate, поэтому вырезание полноэкранных blur-пассов помогает
кратно больше. **BUG-276 закрыт как прямое следствие** — клин свопчейна был
вызван 40 полноэкранными фильтр-пассами/кадр, задушившими iGPU.

**Корректность:**
- 1003 unit-теста `lumen-paint --lib` — pass; clippy `-D warnings` — чист.
- Пиксельная визуальная A/B через spectacle (`30-css-filter.html`, kill-switch
  ON vs OFF): изображения идентичны — все фильтры (blur/grayscale/hue-rotate/
  invert/sepia/drop-shadow/backdrop) рендерятся одинаково.
- Kill-switch ON воспроизводит baseline бит-в-бит по времени (131–133 мс).

Скрипт визуальной A/B: `.tmp/capture_ab.sh <page> <prefix>` (spectacle по
активному окну, KWin-скрипт поднимает окно Lumen; работает на KDE/Wayland).

### Срез 2 — strip+blit скролл-композитор (2026-07-13, WIP, OFF по умолчанию)

Портирован механизм п.19 exp-ветки: `PageBandCache` (персистентная текстура-
полоса в документных координатах) + `RenderPassMode{Normal,Band,Compose}` +
`try_page_compose` + разбиение `render()`→`render_impl(mode)`. При стабильном
контенте (ключ = scroll-инвариантный `hash_display_list` при scroll 0 +
`content_generation`) страница растеризуется в полосу один раз (MISS), а кадры
скролла = blit полосы со сдвигом + overlay (HIT). Guards: только оконный рендер,
`scroll_x==0`, нет sticky. Полоса = вьюпорт + 0.75× запаса (≤768 CSS px).
Инъекция blit-квада — первым op-ом level 0 в Compose. Depth-текстура полосы
подменяется на время Band-рендера. `content_generation` бампается в
`register_image` (lazy/GIF инвалидируют полосу).

**Статус: opt-in `LUMEN_SCROLL_COMPOSITOR=1`, НЕ дефолт — два блокера:**

1. **Баг корректности bbox-scissor × Band.** В Band-рендере полосы bbox-scissor
   фильтр-пассов (Срез 1) роняет blur-элементы: визуальная A/B на
   bench-static-scroll — с `LUMEN_NO_BBOX_SCISSOR=1` blur на месте, с bbox ON
   размытые боксы пропадают. **Точный репро для отладки:** баг есть даже при
   `band_top=0` (scroll 450), значит дело НЕ в смещении полосы, а в том, что при
   большой высоте цели (полоса ~1800 px против окна 720) фильтр получает «тесный»
   scissor в offscreen-полосу и он оказывается неверным (в обычном рендере окна
   тот же бокс у нижнего края даёт `sy1 >= surface_h` → scissor клампится/`full`,
   а в полосе — тесный прямоугольник). Следующий заход: инструментировать
   PopFilter — печать вычисленного `DeviceScissor` в Band vs Normal на одном боксе.
2. **Перф на слабой iGPU скромный и пока невалидный.** Замер ON 10.1 vs OFF
   16.9 мс (bench-static-scroll step 200) снят С багом №1 (пропущенные blur-боксы
   = меньше работы), поэтому это НЕ честное число. Даже без бага полноэкранный
   blit полосы 1024×1800 на Intel HD 530 сам стоит ~10 мс + фикс. per-frame
   overhead `render_impl` (парс шрифтов, image-pre-pass) платится каждый Compose-
   кадр. Ожидаемый потолок здесь — раза в 2, не 6–8× как на Windows.

Инфраструктура (structs/RenderPassMode/render_impl split) на месте и компилится;
1000000-final (анимированная) с композитором ON не меняется — ключ нестабилен,
путь не включается (as designed). `LUMEN_SCROLL_COMPOSITOR=1` для экспериментов.

### Дальше (следующие срезы, по порядку Mozilla)

0. **Фикс бага bbox×Band** (блокер Среза 2) — по репро выше; затем перемер честного
   перфа и включение композитора по умолчанию, если выигрыш оправдан.

1. **viewport-cull невидимых слоёв** (п.17 exp, ещё ~1.27×): Push(Opacity|Blend|
   Filter) запоминает `render_plan.len()`, Pop при невидимом слое делает
   `render_plan.truncate(mark)`. LevelBounds уже считает bbox — видимость даётся
   почти даром. Инфраструктура готова этим срезом.
2. **strip+blit скролл-композитор** (п.19, статика 6–8×): полоса вьюпорт+запас,
   blit со сдвигом по стабильному контенту. Тайлы в координатах scroll root —
   финальная форма (scroll-инвариантный ключ, урок п.15).
3. **static/animated split** (п.21) — медленный скролл анимированных страниц.
