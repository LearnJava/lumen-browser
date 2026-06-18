# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress


---

## Next

### USABILITY-вертикаль — «зашёл на сайт и комфортно пользуешься» (ТОП-приоритет)

Контекст (анализ 2026-06-18): список фич в `CAPABILITIES.md` широкий (~90 Web-API,
сеть/layout работают на живых сайтах — github.com/HN грузятся через `--dump-layout`),
но **вертикаль повседневного использования разорвана**. Эти задачи закрывают разрыв; они
важнее экзотики из PH3. Owner проставлен — часть передаётся P2/P3/P4 (см. их STATUS).

| # | Задача | Owner | Размер | Крейты / точки в коде |
|---|--------|-------|--------|----------------------|
| **U-0** | ~~**`--screenshot <out.png> <url>` headless CPU-снимок**~~ ✅ завершена (2026-06-18) — инструмент наблюдения: честная картинка любого сайта без окна/Edge/ffmpeg. См. «Recent merges». | P1 | S | `lumen-shell` |
| **U-1** | **Неблокирующая загрузка страницы** — ~~этап 1~~ ✅ (2026-06-18): навигация (клик, адресная строка, back/forward, JS `location.href=`, reload) больше не мёрзнет — `reload()` идёт через тот же async streaming-пайплайн, что и первичная загрузка (`start_streaming_load`), окно рисует промежуточные кадры, тяжёлый `render_bytes` исполняется один раз в `LoadDone`. Добавлены load-generation guard (отброс устаревших событий гонки навигаций) и `pending_restore_scroll` (восстановление scroll для back/forward при асинхронном reload). **Остался этап 2 (ADR-006): весь финальный пайплайн (включая QuickJS) вне UI-потока — заблокирован тем, что QuickJS не `Send`.** Перекрывается с BUG-171/BUG-172. | P1 | XL (этап 1 ✅) | `lumen-shell` `main.rs` (`reload`/`start_streaming_load`/`apply_loaded_page`); `lumen-network` |
| **U-2** | **Шейпинг текста (GSUB/GPOS) + CFF-контуры.** ~~Этап 1: GPOS-кернинг + GSUB-лигатуры~~ ✅ 2026-06-18. ~~Этап 2: CFF-контуры~~ ✅ 2026-06-18 (`lumen-font::cff` — Type 2 charstrings, CID-keyed CFF, проводка через `Font::glyph_resolved` → CPU/wgpu/Canvas рисуют `.otf`-текст). См. «Recent merges». Опционально позже: CFF2 (variable PostScript), проводка шейпинга в femtovg live-окно и per-char measurement. | P1 | XL ✅ | `lumen-font` (`cff.rs`) |
| **U-3** | **Настоящая многовкладочность** — TAB-серия ниже. **Бо́льшая часть уже в коде** (ревизия 2026-06-18): TAB-1/2/3/6 реализованы (`PageSnapshot`+`bg_tabs`, `switch_tab`/`open_new_tab`/`close_tab`, tab-bar UI, hibernation, Ctrl+T/W/Tab). TAB-4 ✅ / TAB-5 ✅ (IPC, 2026-06-18). TAB-7 ✅ (run.py `--ipc`, 2026-06-18). | P1 | см. TAB | `lumen-shell` |
| **U-4** | **WASM-исполнение + закрыть бросающие JS-заглушки** — современные SPA белеют, наткнувшись на `reject` (WebGPU/WebCodecs) или невыполнимый WASM (сейчас только проверка magic-байтов). Этап 1: интерпретатор WASM MVP; этап 2: аудит стабов, которые должны degrade-gracefully, а не throw. | P1 | XL | `lumen-js` (`wasm.rs`, web_codecs/webgpu шимы) |
| **U-5** | **Fullscreen пересчитывает вьюпорт** — BUG-167: вход в Fullscreen растягивает окно, но страница остаётся ~1024×720. | P3 | S | `lumen-shell` `main.rs:6400` |
| **U-6** | **Рендер-паритет high-deviation** — добить заметные отклонения: repeating-gradient (BUG-085), filter/backdrop (BUG-144), anchor-positioning (BUG-126 53%), scroll-snap (BUG-104 64%). | P3/P4 | — | см. `BUGS.md` |

**Порядок для P1:** U-0 ✅ → U-1 этап 1 ✅ → U-3 (TAB-серия) ✅ → U-2 этап 1 (GSUB/GPOS шейпинг) ✅ → U-2 этап 2 (CFF-контуры) ✅ 2026-06-18 → **U-4 (WASM, ранее у P2)**. U-1 этап 2 (пайплайн вне UI) — после разблокировки QuickJS `!Send`.

**Как мерить прогресс:** после каждого этапа снимай `--screenshot` 5–10 живых сайтов (github, новостной, блог, SPA) + `python graphic_tests/run.py --build --continue-on-fail` для CSS-паритета.

---

### TAB-series — Multi-tab support + screenshot IPC (приоритет)

Цель: один процесс Lumen держит несколько вкладок с изолированным состоянием; `run.py` открывает браузер один раз и получает скриншоты через IPC без gdigrab.

**Статус (ревизия 2026-06-18):** интерактивная многовкладочность в окне **уже реализована**
(вопреки прежнему описанию). В `Lumen` per-tab состояние вынесено в `PageSnapshot`
(`main.rs:3303`), фоновые вкладки в `bg_tabs: HashMap<usize, PageSnapshot>`; есть
`switch_tab`/`open_new_tab`/`close_tab`/`save_page_snapshot`/`restore_page_snapshot`,
tab-bar UI (`tabs::strip`), пятиуровневая lifecycle-модель (`tab_lifecycle/`, T0→T4,
hibernation в SQLite), биндинги Ctrl+T/Ctrl+W/Ctrl+Tab. Поэтому **TAB-1/2/3/6 закрыты кодом**
(модель — swap `PageSnapshot`, а не `Vec<TabState>`, но цель «per-tab изоляция» достигнута).

| # | Задача | Размер | Крейты | Статус |
|---|--------|--------|--------|--------|
| TAB-1 | **PageState extraction** — per-tab состояние. | M | `lumen-shell` | ✅ готово (`PageSnapshot`+`bg_tabs`) |
| TAB-2 | **Tab switching** — Ctrl+T/W/Tab, swap активной вкладки. | S | `lumen-shell` | ✅ готово (`switch_tab`) |
| TAB-3 | **Tab bar UI** — полоса вкладок, заголовок, крестик, `+`. | M | `lumen-shell` | ✅ готово (`tabs::strip`) |
| TAB-4 | **IPC tab routing** — `TabId` + `CreateTab`/`CloseTab`/`NavigateTab`. | S | `lumen-ipc`, `lumen-shell` | ✅ 2026-06-18 |
| TAB-5 | **Screenshot IPC** — `Screenshot(tab_id)` → PNG offscreen (CPU). | S | `lumen-ipc`, `lumen-shell` | ✅ 2026-06-18 |
| TAB-6 | **Подключить TabLifecycleManager** — idle T0→T2 по таймаутам. | S | `lumen-shell` | ✅ готово (`tab_lifecycle/`) |
| TAB-7 | **run.py IPC-режим** — `lumen.exe --ipc-server`, `NavigateTab + Screenshot` на тест, PNG vs Edge, убрать gdigrab. **Требует Python-реализацию bincode** для `lumen_ipc::{IpcRequest,IpcResponse}` (variant-tag u32 LE + u64 LE длины строк/Vec). | XS | `graphic_tests/run.py` | ✅ 2026-06-18 (опц. `--ipc`; полная замена gdigrab — после BUG-221) |

**Следующий шаг:** U-2 (шейпинг текста GSUB/GPOS + CFF-контуры) по основному порядку P1.
TAB-7 закрыт флагом `--ipc` (Python-клиент bincode к `--ipc-server` работает, протокол
верифицирован пиксельно на гео/цвет/текст-тестах). **Полностью убрать gdigrab нельзя до
паритета CPU-бэкенда снимка** (BUG-221: `render_to_image_cpu` не рисует border-radius/
gradients/images как femtovg), поэтому gdigrab пока дефолт, `--ipc` — опция.

---

### Унаследовано от P2 (P2 → резерв, 2026-06-18)

P2 выведен в резерв; все его незакрытые задачи теперь у P1. Приоритет ниже
USABILITY-вертикали и TAB-серии — брать после них или когда они заблокированы.

**Незакрытые task-файлы (готовые брифы в `docs/tasks/`):**

| # | Задача | Размер | Крейты | Бриф |
|---|--------|--------|--------|------|
| P2→1 | **Tab tier tooltip при hover (10K.2)** — tooltip «вкладка спит — клик восстановит» на бейдже tier. | S | `lumen-shell` | `docs/tasks/p2-tab-tier-tooltip.md` |
| P2→2 | **Loading spinner при restore гибернированной вкладки >200ms (10K.3)** — overlay-кольцо на время восстановления из SQLite. | S | `lumen-shell` | `docs/tasks/p2-tab-restore-spinner.md` |
| P2→3 | **FemtovgBackend — PushFilter blur** — реальный box-blur (сейчас только save/restore, размытия нет). | M | `lumen-paint` | `docs/tasks/p2-femtovg-filter-blur.md` |
| P2→4 | **FemtovgBackend — истинные эллиптические border-radius** — сейчас `max(rx,ry)` рисует круг вместо эллипса. | S | `lumen-paint` | `docs/tasks/p2-femtovg-elliptical-radius.md` |

**Направления без брифа (бывшие «Опции» P2):**

- **Ad-block Phase 2/3** — `$option` по типу ресурса (Phase 2) + UI подписок (Phase 3, handoff P3). База — `p2-adblock-filter-lists` (влита 2026-06-16). Крейты: `lumen-network`, `lumen-shell`.
- **Canvas Phase extensions** — Canvas 2D text-completeness (BUG-099 continuation), advanced patterns/shadows refinement. Крейты: `lumen-canvas`, `lumen-js`.
- **Color management Phase 1+** — Lab/CMYK color spaces (H-2 Phase 1+), device-specific tone curves, advanced ICC. Крейты: `lumen-image`, `lumen-paint`.
- **DevTools network-panel: `Event::RequestFailed`** (бывшая задача #30, ранее handoff P3→P2) — отрисовка проваленных запросов в `devtools/network_panel.rs`. Крейт: `lumen-shell`.

> Баги высокого отклонения (BUG-085 gradient, BUG-088 transforms, BUG-090 line-clamp),
> которые в STATUS-P2 числились как «P3-вспомога», остаются у **P3** — это их домен
> по CLAUDE.md «Bug ownership: P3 only», в P1 не переносятся.

---

### Правило: фиксировать реализованное

После каждой задачи **в том же коммите** обновлять:
1. `STATUS-P1.md` — переместить из `Next` в `Recent merges` с кратким описанием
2. `CAPABILITIES.md` — изменить ⬜/🟡 → ✅ для реализованных возможностей
3. `subsystems/<crate>.md` — добавить bullet в раздел **Done**

Это предотвращает повторную реализацию уже готового другими сессиями.

---

### Streaming rendering — оставшиеся дыры (приоритет, до PH3)

PH1-2 закрыл только window-first + 60 Hz throttle + параллельную загрузку CSS. Реальная
потоковая отрисовка «по мере прихода из сети» ещё не работает. Три задачи по убыванию
заметности для пользователя:

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH1-2a | ~~**TCP-level streaming HTTP body**~~ ✅ завершена (2026-06-16) — `HttpClient::fetch_page_streaming(url, on_chunk)` отдаёт декодированные порции тела по мере чтения сокета; shell `start_streaming_load` шлёт реальные сетевые чанки. | L | `lumen-network`, `lumen-shell` |
| PH1-2b | ~~**Инкрементальный (dirty-subtree) layout**~~ ✅ завершена (2026-06-16) — `layout_streaming_incremental` переиспользует геометрию неизменённого префикса из прошлого кадра, релейаутит лишь новые/изменённые поддеревья; `paint_partial_dom` гейтит через `stream_layout_seeded`. | L | `lumen-layout`, `lumen-shell` |
| PH1-2c | ~~**Прогрессивная подгрузка картинок во время streaming**~~ ✅ завершена (2026-06-16) — `paint_partial_dom` спавнит параллельные fetch+decode для `<img>` частичного DOM, картинки дорисовываются по приходу через `LoadEvent::ImageDecoded`. | M | `lumen-image`, `lumen-shell` |

### PH3 — Phase 3: v1.0 «Full Browser»

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH3-1 | ~~**DevTools Elements styled-rules panel**~~ ✅ завершена | M | `lumen-shell` (devtools/) |
| PH3-3 | ~~**getUserMedia Phase 1**~~ ✅ завершена | L | `lumen-js`, `lumen-shell` |
| PH3-4 | ~~**Offscreen Canvas Phase 1**~~ ✅ завершена | M | `lumen-js`, `lumen-paint` |
| PH3-5 | ~~**Web Workers Phase 1**~~ ✅ завершена | L | `lumen-js` |
| PH3-9 | ~~**HTML5 Drag and Drop API**~~ ✅ завершена | M | `lumen-dom`, `lumen-js`, `lumen-shell` |
| PH3-11 | ~~**`<audio>` element Phase 1 — HTMLAudioElement real playback**~~ ✅ завершена | L | `lumen-core`, `lumen-js`, `lumen-shell` |
| PH3-12 | ~~**`<video>` element Phase 1 — HTMLVideoElement GIF playback**~~ ✅ завершена | M | `lumen-js`, `lumen-paint`, `lumen-shell` |
| PH3-13 | ~~**Screen Wake Lock API Phase 1**~~ ✅ завершена | M | `lumen-core`, `lumen-js`, `lumen-shell` |

---

## Recent merges

| Дата | Задача | Описание |
|------|--------|---------|
| 2026-06-18 | U-2 этап 2: CFF-контуры (PostScript-OpenType `.otf`) | `.otf`-шрифты с PostScript-контурами (`'OTTO'` sfnt, таблица `CFF ` вместо `glyf`/`loca`) не рисовали глифы ни на одном пути рендера. Новый модуль `lumen-font::cff`: `Cff::parse` разбирает структуру CFF (header → Name/TopDICT/String/GlobalSubr INDEX → CharStrings INDEX, Private DICT + local Subr INDEX), `Cff::glyph(gid)` интерпретирует Type 2 charstring в `Outline::Simple`. Кубические безье флэттятся в on-curve отрезки (`CUBIC_STEPS=10`), поэтому переиспользуется существующий квадратичный растеризатор без изменений; bbox вычисляется из флэттенных точек. Интерпретатор: все path-операторы (rmoveto/h/v, rlineto, h/vlineto, rrcurveto, rcurveline/rlinecurve, vv/hhcurveto, vh/hvcurveto), hint-операторы (hstem/vstem/-hm считаются, чтобы пропустить байты маски `hintmask`/`cntrmask`), `callsubr`/`callgsubr`/`return` со стандартным subr-bias (107/1131/32768, глубина ≤10), четыре flex-оператора, опциональный leading-width операнд, `endchar` с `seac`-композитами. **CID-keyed CFF:** `ROS`/`FDArray`/`FDSelect` (форматы 0+3) → per-FD local subrs. Проводка: `Font::cff()`/`has_cff()` + ранняя ветка в `Font::glyph_resolved`/`glyph_resolved_with_coords` — CPU-растеризатор, wgpu-renderer (`renderer.rs`) и Canvas 2D рисуют `.otf`-текст прозрачно, без изменений в потребителях. CFF v1 не имеет variation-дельт, поэтому `glyph_resolved_with_coords` для CFF игнорирует `coords` (CFF2 отложен). Проверка: 16 unit-тестов на рукотворной spec-valid CFF-таблице (INDEX/DICT/real/bias/квадрат/кривая/subr/hintmask/width/hv-lineto/FDSelect); dev-валидация против реальных системных OTF (AdobeClean — non-CID, MyriadCAD — CID-keyed) рендерит читаемые `R`/`g`/`A`. Реальный CFF-шрифт в репо не закоммичен (нет permissive `.otf` под рукой) — committed-тесты синтетические, но прогоняют полный парс+интерпрет на корректных байтах. clippy чисто, 354 font-теста. Вне этапа: CFF2 (variable PostScript), charstring arithmetic-операторы. |
| 2026-06-18 | U-2 этап 1: шейпинг текста (GSUB-лигатуры + GPOS-кернинг) | Текст на CPU-пути рисовался по одному глифу на символ, без OpenType Layout — ни кернинга, ни лигатур. Реализован движок шейпинга в `lumen-font` и подключён к CPU-растеризатору (путь `--screenshot` + snapshot-гейт), где `lumen-font` — авторитет рендера текста. Новые модули: `otlayout` (общий слой GSUB/GPOS — заголовок + навигация script→langsys→feature→lookup через `enabled_lookups`, политика DFLT→latn→cyrl; `Coverage` fmt 1/2, `ClassDef` fmt 1/2, `ValueRecord`, `resolve_extension`); `gsub` (Type 1 single fmt 1/2, Type 4 ligature fmt 1 — жадно, кластер лигатуры = min компонент, Type 7 extension; фичи `liga`/`clig`/`calt`/`rlig`/`ccmp` — `calt` включён, т.к. Inter не имеет `liga`, а стрелки/лигатуры лежат в `calt` type-4); `gpos` (Type 1 single fmt 1/2, Type 2 pair fmt 1 пары-глифов + fmt 2 пары-классов, Type 9 extension; фича `kern`); `shape` (`Shaper::shape(glyph_ids, hmtx) -> Vec<ShapedGlyph>` — GSUB → сид base advance из hmtx → GPOS; без таблиц деградирует к base advance, вывод идентичен прежнему). Проводка: `lumen-paint::cpu_raster::rasterize_text` шейпит по сегментам между табами, рисует глифы со сдвигами и шейпленными advance. **Намеренно не трогаются:** per-char measurement (layout) и live-окно femtovg (femtovg шейпит сам через `fill_text`) — это перекрывает headline-цель «текст в окне», но требует переписать femtovg-текст glyph-by-glyph (отдельная опция). Вне этапа 1: контекстные lookups (GSUB 5/6, GPOS 7/8), mark-позиционирование (GPOS 3–6), сложные скрипты, mark filtering, CFF-контуры (этап 2). Проверка: 14 unit + 7 интеграционных (`inter_shaping.rs`: GPOS кернит AV/To/Wa/Type, GSUB лигатит `->`→стрелка); сквозной `--screenshot` подтверждает `x->y`→`x→y` + кернинг; clippy чисто, 339 font + 792 paint тестов. Гейт `snapshot_cpu` (feature-gated) уже красный на main из-за дрейфа геометрии (BUG-221) — baseline не перегенерирован. |
| 2026-06-18 | TAB-7: run.py IPC-режим (`--ipc`) | `graphic_tests/run.py` получил флаг `--ipc` — захват Lumen через `lumen.exe --ipc-server` вместо окна+gdigrab. Чистая Python-реализация bincode-протокола `lumen_ipc` (без зависимостей): `LumenIpcClient` спавнит сервер, парсит `LUMEN_IPC_PORT=<port>` из stdout, дренирует остаток stdout фоновым потоком (иначе рендер-логи переполнят пайп → блок шелла, как в `crates/shell/tests/ipc_server.rs`), подключается по TCP loopback и шлёт length-prefixed bincode: variant-tag u32 LE, длины String/Vec u64 LE, u32-поля 4 байта LE. Команды `CreateTab`/`NavigateTab(abs_path)`/`Screenshot`→PNG/`Shutdown` (завершение через `atexit`). Одна вкладка переиспользуется на все тесты. CPU-снимок детерминирован и начинается с (0,0) — магента-калибровка и crop offset не нужны (`crop_offset=(0,0)`), Edge-эталон + ffmpeg-crop + diff-метрика прежние. Верифицировано: TEST-00/01/03/06/08/09/13/22 PASS 0.00–0.22% (пиксельный паритет геометрии/цвета/текста/transform, лучше gdigrab-шума). **Ограничение:** CPU-бэкенд снимка (`render_to_image_cpu`) пока не на паритете с femtovg по border-radius (TEST-36 квадрат), gradients (39), images (18) — заведён **BUG-221**; поэтому `--ipc` опционален, gdigrab остаётся дефолтом до закрытия BUG-221. Затрагивает только `graphic_tests/run.py` (test-infra, Rust не менялся). |
| 2026-06-18 | TAB-4 + TAB-5: IPC tab control + Screenshot | `lumen-ipc` расширен таб-командами (`TabId` + `IpcRequest::{CreateTab,CloseTab,NavigateTab,Screenshot}` / `IpcResponse::{TabCreated,TabClosed,Navigated,Screenshot,TabError}`). Новый headless-режим шелла `--ipc-server` (`CliMode::IpcServer`, `run_ipc_server`): шелл становится TCP-сервером таб-команд, печатает `LUMEN_IPC_PORT=<port>` в stdout, держит `HashMap<TabId, PageSource>`. «Вкладка» — headless-контекст рендера; `Screenshot` лениво гоняет тот же CPU-пайплайн, что `--screenshot` (рефактор `do_screenshot` → переиспользуемый `render_source_to_png`), без winit/wgpu — детерминированный PNG для CI/run.py. Состояние вкладок переживает переподключения; сервер выходит по `Shutdown`. Тесты: round-trip в `lumen-ipc` (5/5) + интеграционный `crates/shell/tests/ipc_server.rs` (спавнит реальный бинарь, гоняет create→navigate(file)→screenshot→close→shutdown, проверяет PNG-magic). Это TAB-4/TAB-5 минимального пути; TAB-7 (run.py-клиент с bincode на Python) — следующая. Ревизия показала: TAB-1/2/3/6 уже были в коде (`PageSnapshot`/`bg_tabs`/`switch_tab`/tab-bar/lifecycle), STATUS подправлен. |
| 2026-06-18 | U-1 (этап 1): неблокирующая навигация | `reload()` больше не гоняет весь fetch+parse+JS+layout синхронно на UI-потоке. Когда окно есть (любая навигация после первого кадра — клик по ссылке, адресная строка, back/forward, JS `location.href=`, reload палитры/tab-bar), сбрасываем streaming-состояние и делегируем в `start_streaming_load()` — тот же async-путь, что и первичная загрузка в `resumed()`: HTML стримится в фоновом потоке, окно рисует промежуточные кадры, тяжёлый финальный `render_bytes` исполняется один раз на UI-потоке в `LoadEvent::LoadDone` (`apply_loaded_page`). Синхронный fallback (прежний GpuSession/`source.load`) сохранён для пути без окна (headless/тесты). **Load-generation guard:** `Lumen.load_generation` инкрементится на каждую навигацию и метит все streaming-события (`EarlyPreloadHints`/`HtmlChunk`/`CssLoaded`/`LoadDone`/`LoadError`); `user_event` отбрасывает события устаревшего поколения — медленная вытесненная загрузка не подмешает DOM/CSS прошлой страницы и не нарисует её поверх новой (гонка, ставшая достижимой из-за того, что UI больше не блокируется). **Scroll-restore:** back/forward (и bfcache) стэшат offset в `Lumen.pending_restore_scroll` перед `reload()`, а `apply_loaded_page` применяет его после сброса scroll в 0 — прежний код ставил `scroll_x/y` сразу после (тогда синхронного) `reload()`, что при асинхронном `LoadDone` затёрлось бы. Остался этап 2 (ADR-006: финальный пайплайн, включая QuickJS, вне UI-потока — блокер: QuickJS не `Send`). Проверка: `cargo check`/clippy clean, 1333 теста шелла, graphic TEST-00/01/03/06/20 PASS, headless `--screenshot example.com` рендерит. |
| 2026-06-18 | U-0: `--screenshot <out.png> <url>` headless CPU-снимок | Новый CLI-режим шелла: `lumen --screenshot out.png <path-or-url>` гоняет полный headless-пайплайн (`source.load_bytes` → `parse_and_layout` с внешним CSS/картинками → `paint_ordered`) и растеризует display-list детерминированным CPU-бэкендом (`Renderer::render_to_image_cpu`, feature `cpu-render`, tiny-skia) → PNG. Без wgpu/winit/окна, без Edge и ffmpeg, пиксельно воспроизводимо на любой ОС. Высота снимка = высота layout-корня (`parsed.layout.rect.height`), зажата в `[720, 32768]` (полная страница, не только первый экран), ширина 1024. `extract_screenshot()` зеркалит `extract_print_to_pdf` (порядок `--screenshot <out> <url>`); новый `CliMode::Screenshot` + `run_screenshot`/`do_screenshot`. Шелл теперь включает `lumen-paint/cpu-render` в дефолте. Попутно: `#[allow(clippy::too_many_arguments)]` на `draw_border_side_h/v` в `cpu_raster.rs` (8 геом-параметров, открылись для строгого clippy при включении cpu-render). 4 unit-теста `extract_screenshot`. Инструмент наблюдаемости для USABILITY-вертикали (см. Next). |
| 2026-06-16 | PH3-20: Service Worker Fetch Interception Phase 1 | Активный SW исполняется в выделенном QuickJS-потоке (`lumen-js::sw_worker::spawn_sw_worker`): `ServiceWorkerGlobalScope` + `caches`/`Headers`/`Response` шим, реальные base64 `atob`/`btoa`, `install`/`activate`/`fetch` события. На фазе активации `_sw_run_lifecycle` фетчит скрипт SW и зовёт `_lumen_sw_activate_script(origin, scope, text)` → спавн потока, регистрация в `SwWorkerStore` (`(origin,scope)→SwWorkerHandle`). `ServiceWorkerInterceptor` (lumen-storage) маршрутизирует fetch через `sw_worker_store` со scope-prefix matching (longest-match), независимо от SQLite-регистраций (shell хранит их in-memory); диспатчит `FetchEvent`, ждёт `respondWith()` (5 c таймаут), отдаёт тело сети. Shell: общий in-memory `cache_store: Arc<CacheStorage>` передаётся в `install_dom` (страница + SW-поток делят кэш) и в глобальный `SW_FETCH_INTERCEPTOR`, подключаемый в `http_client_for_subresource`. Новые типы в `lumen-core::ext`: `SwFetchRequest`/`SwWorkerHandle`/`SwWorkerStore`. Ограничение Phase 1: внутри SW `fetch` — только cache-first (без сети), поэтому `cache.addAll()` прекэш из сети не работает; SW отдаёт лишь то, что закэшировала страница. 9 тестов sw_interceptor (5 SQLite + 4 worker-routing), 5 тестов sw_worker. Гочи: `rt.execute_pending_job()` нельзя вызывать внутри `ctx.with(...)` (реентрантность рантайма → паника rquickjs) — `flush_jobs` всегда вне `ctx.with`. |
| 2026-06-16 | PH3-19: font-display: swap — неблокирующая загрузка web-шрифтов | `load_font_faces` разделена на local()-sync + url()-async. `ParsedPage`/`LoadedPage` несут `pending_web_fonts: Vec<PendingWebFont>`; `apply_loaded_page` спавнит по фоновому потоку на каждый: fetch → WOFF-декод → sfnt-валидация → `LoadEvent::FontLoaded`. Обработчик на UI-потоке: `page_font_registry.register_from_bytes`, push в `web_fonts`, `relayout_page(..., &self.web_fonts)` строит `MultiFontMeasurer` из накопленных шрифтов (FOUT swap), `request_redraw`. BUG-170 закрыт. |
| 2026-06-16 | PH1-2c: прогрессивная подгрузка картинок во время streaming | `paint_partial_dom` после layout зовёт `spawn_stream_image_loads(doc, viewport)` — `collect_image_requests` по частичному DOM, для каждого не-lazy и ещё-не-запрошенного `src` спавнится поток `fetch_image_bytes`+`decode`/`decode_gif_animated`. По завершении поток шлёт `LoadEvent::ImageDecoded { src, image, animated }`; `user_event` регистрирует картинку в renderer-е (или `pending_images` до создания окна), кладёт в `image_cache`, анимированный GIF — в `animated_gifs` (тик в `RedrawRequested`), и просит redraw — картинки появляются по мере прихода, как CSS, а не разом в финальном `LoadDone`. Дедуп через новое поле `stream_images_requested: HashSet<String>` (сбрасывается на каждую навигацию; сохраняется/восстанавливается в `PageSnapshot`). `PageSource::resource_base()` — общий хелпер базы подресурсов. 3 новых shell-теста (итого 1326). |
| 2026-06-16 | PH1-2b: инкрементальный streaming-layout | `layout_streaming_incremental(doc, sheet, vp, m, hp, dark, prev)` строит свежее box-дерево из выросшего DOM и переиспользует геометрию из `prev` для поддеревьев с неизменными node-id/BoxKind-payload/ComputedStyle; релейаут только новых/изменённых поддеревьев через `lay_out_incremental`, неизменённый префикс репозиционируется за O(1). Layout: `incremental::mark_subtree_dirty` + `graft_geometry` (рекурсивное сопоставление по индексу, `kind_layout_eq`+`segments_eq` детектят дописываемый текст в InlineRun). Shell: `paint_partial_dom` гейтит graft через `stream_layout_seeded` (первый кадр навигации — полный layout-засев, чтобы не переиспользовать геометрию прошлой страницы). Тесты: геометрия инкремент-прохода совпадает с полным layout (append-блок + reflow абзаца) + unit graft. |
| 2026-06-16 | PH1-2a: TCP-level streaming HTTP body | `HttpClient::fetch_page_streaming(url, on_chunk)` отдаёт декодированные порции тела по мере чтения сокета — браузер начинает парсить/рисовать HTML до полного скачивания. Network: `read_response_streamed` + `BodyReader` (Content-Length / chunked / read-to-EOF Read-адаптер) + `TeeReader` + `detect_stream_decode`; streaming-decode для identity/br/gzip/deflate (br/gzip/deflate через streaming `Read`-декодеры, br — `brotli_decompressor::Decompressor`, остальные `flate2`), gated на финальный 2xx; sink проброшен через `do_request`/`fetch_single`/`fetch_with_redirect` (`ChunkSink` алиас). Возвращаемое тело — полное декодированное (как `fetch_page`), sink — best-effort preview. Shell: `PageSource::load_bytes_streaming` + `start_streaming_load` для URL-источников шлёт реальные сетевые чанки (File/Snapshot/Static — прежняя нарезка буфера); `feed_preload_and_emit` объединяет preload-scan обоих путей. 9 новых network-тестов. Попутно (BUG-168) — Linux-unblock pre-existing clippy/test-сбоев в platform-cfg коде (ctap2 ×3, screen_capture/file_dialog ×2). |
| 2026-06-16 | PH3-18: Pointer Lock Phase 1 | `pending_grab` флаг в `pointer_lock.rs` + `take_pending_grab()` для shell; `_ptr_lock_el` JS-переменная для `pointerLockElement` getter; `_lumen_dispatch_locked_mousemove()` — mousemove+pointermove с movementX/Y; `device_event()` в shell → `DeviceEvent::MouseMotion` → `_lumen_dispatch_locked_mousemove`; `about_to_wait` drain `CursorGrabMode::Locked`/`None`; Escape освобождает lock; `CursorMoved` при locked подавляется. 10 новых тестов. |
| 2026-06-16 | PH3-17: Screen Capture API Phase 1 | `ScreenCaptureProvider` трейт + `NullScreenCaptureProvider` в lumen-core::ext; `VideoFrame` struct; `__lumen_screen_capture_{list_sources,start,info,read_frame,stop}` нативные биндинги + `set_screen_capture_provider()` в lumen-js; `getDisplayMedia()` резолвится с живым `MediaStream` + video track + `readVideoFrame()`; `PlatformScreenCapture` (Win32 GDI `BitBlt`/`GetDIBits` + BGRA→RGBA) в shell/src/platform/screen_capture.rs. 14 новых тестов (3 lumen-core + 11 lumen-js). |
| 2026-06-16 | PH3-16: Idle Detection API Phase 1 | `__lumen_idle_get_idle_ms()` → Win32 `GetLastInputInfo+GetTickCount` на Windows, 0 на Linux/macOS; `IdleDetector.start()` запускает `setInterval(max(30s, threshold/2))`, диспатчит `'change'` при переходе `userState` active↔idle; `#[link(name = "user32")]`. 16 новых тестов. |
| 2026-06-16 | PH3-15: File System Access API Phase 1 | `showOpenFilePicker/showSaveFilePicker/showDirectoryPicker` → Promise; `FileSystemFileHandle`/`FileSystemDirectoryHandle`/`FileSystemWritableFileStream` JS-классы; `WriteRegistry` (append + flush-on-close); `DirRegistry`; OS диалоги WinForms/zenity/osascript; токен-безопасность через PH3-14 `register_file_token`. 33 новых теста lumen-js. |
| 2026-06-16 | PH3-14: File Input API Phase 1 | `register_file_token()` + thread-local `FILE_REGISTRY`; нативные биндинги `__lumen_file_read_text`/`__lumen_file_read_base64`; `File.prototype.text()`/`arrayBuffer()`/`stream()` читают реальные байты через токены; `entries_to_json_with_tokens()` в shell; JS не видит сырых путей файловой системы. 18 новых тестов lumen-js + 4 lumen-shell. |
| 2026-06-16 | PH3-13: Screen Wake Lock API Phase 1 | `WakeLockProvider` трейт + `NullWakeLockProvider` в lumen-core::ext; `set_wake_lock_provider()` + `__lumen_wake_lock_request`/`__lumen_wake_lock_release` биндинги + обновлённый JS-шим в lumen-js; `PlatformWakeLock` (`SetThreadExecutionState` на Windows, no-op на Linux/macOS) в shell/src/platform/wake_lock.rs. 23 новых теста. |
| 2026-06-16 | PH3-12: `<video>` element Phase 1 | `VideoGifStore` + `VideoPlaybackState` в lumen-js (без зависимости от lumen_image); 12 нативных биндингов `__lumen_video_*` + JS-шим; `BoxKind::Video` → `DrawImage { src: "video:{nid}" }` в display_list; `tick_video_gifs()` в shell — декодирует GIF, регистрирует кадры, продвигает анимацию. |
| 2026-06-16 | PH3-11: `<audio>` element Phase 1 | `AudioPlaybackProvider` трейт в lumen-core; 16 нативных биндингов `__lumen_audio_*` + JS-шим (play/pause/seek/timeupdate/ended/loop/canPlayType) в lumen-js; `PlatformAudioPlayer` на rodio 0.19 с per-handle audio thread + mpsc в lumen-shell. 39 новых тестов. |
| 2026-06-16 | PH3-10: Pointer Events API Level 3 | `pointer_captures` HashMap в lumen-dom; `pointer_capture.rs` Rust-биндинги + `pointer_capture_nid` Arc в lumen-js; `Element.setPointerCapture/releasePointerCapture/hasPointerCapture` + `gotpointercapture`/`lostpointercapture`; L3 свойства (altitudeAngle, getCoalescedEvents); shell routing + implicit release на pointerup. 10 новых тестов, итого 2091 lumen-js. |
| 2026-06-16 | PH3-9: HTML5 Drag and Drop API | `is_element_draggable()` в lumen-dom (HTML LS §9.3.3); `DndState` + `DND_THRESHOLD` + `js_drag_event()` в shell; полный lifecycle: dragstart→drag/dragover/dragenter/dragleave→drop/dragend. 231 тест lumen-dom, 2081 lumen-js. |
| 2026-06-16 | PH3-8: Web Animations API Level 1 (JS runtime) | `DocumentTimeline`, `KeyframeEffect`, `Animation` (play/pause/cancel/finish/reverse), `AnimationPlaybackEvent`; `element.animate()` + `element.getAnimations()`; `document.timeline` + `document.getAnimations()`; интерполяция (числа/цвета/transform), easing (linear/ease/cubic-bezier/steps), fill/direction/iterations. 21 тест. |
| 2026-06-16 | PH3-7: `contentEditable` + Input Events Level 2 + Selection routing | `node_is_contenteditable()`, `find_editing_host()` в lumen-dom; 5 Rust-биндингов + JS-свойства (`contentEditable`, `isContentEditable`) + `_lumen_handle_contenteditable_key()` в lumen-js; маршрутизация клавиш в shell через DOM (не eval_js). 17 новых тестов. |
| 2026-06-16 | PH3-6: `<dialog>` focus management + `<form method="dialog">` | `showModal()` фокусирует [autofocus]-потомок или сам диалог; `close()` восстанавливает предыдущий фокус; `<form method="dialog">` закрывает родительский `<dialog>`. `find_ancestor_dialog()` в lumen-dom. 8 новых тестов. |
| 2026-06-15 | PH3-5: Web Workers Phase 1 | `importScripts()` для data: и blob:lumen/ URL; `WorkerBlobStore` (Arc-shared); `atob`/`btoa` в worker globals; WORKER_SHIM оборачивает createObjectURL для auto-регистрации blob'ов. 20 новых тестов, итого 47 worker-тестов. |
| 2026-06-15 | PH3-4: Offscreen Canvas Phase 1 | `create_offscreen_from_pixels()` + `transferControlToOffscreen()` + `postMessage(data,[transfer])` с сериализацией OffscreenCanvas через сентинели. 8 новых тестов. |
| 2026-06-15 | PH3-3: getUserMedia Phase 1 | `AudioCaptureProvider` + `PlatformAudioCapture` (cpal/WASAPI/ALSA); `__lumen_start_audio_capture` + JS MediaStreamTrack. 247 тестов. |
| 2026-06-15 | PH3-2: `lumen-bidi-server` standalone крейт | WebDriver BiDi сервер вынесен из `shell/src/bidi/` в отдельный крейт. `lumen_bidi_server::spawn` — единственный публичный API. 89 тестов. |
| 2026-06-15 | PH3-1: DevTools Styles-таб | `ComplexSelector::to_css_str()`, `matched_rules_for_node()`, `InspectorTab::Styles` — CSS правила для выбранного узла в DevTools. 16 новых тестов. |
| 2026-06-15 | 9F.3: Tor circuit (`--tor` CLI) | `extract_tor_mode()` + `check_tor_connectivity()` + override `FingerprintProfile` → TorBrowser + `socks5://127.0.0.1:9050` + `no_persistent_state`. Завершает ADR-007 (все 6 слоёв). 6 тестов. |
| 2026-06-15 | PH2-7: Accessibility tree + platform bridges Phase 1 | `WinUiaBridge` Phase 1: `init_hwnd()` + `NotifyWinEvent` (EVENT_OBJECT_FOCUS/REORDER/STATECHANGE) + `handle_wm_get_object` + `ax_role_to_msaa()` (60 вариантов). 125 тестов в lumen-a11y. |
| 2026-06-15 | PH2-3: Профили + шифрование | `profile_vault` — AES-256-GCM key wrapping, PBKDF2-HMAC-SHA256 (100k iter). `ProfileRegistry`: `set_password`, `clear_password`, `unlock`, `is_encrypted`. 11 unit-тестов. |
| 2026-06-15 | PH2-2: Site isolation Phase 1 | `lumen-network::coop` — COOP/COEP/CORP парсинг; 27 тестов. `window.crossOriginIsolated` + pipeline wiring. |
| 2026-06-15 | PH1-8: Preload scanner | `PreloadScanner` struct поверх `PushTokenizer`; инкрементальный scan. 35 тестов. |
| 2026-06-15 | PH1-7: Compositor thread + Property Trees | `InProcessCompositor` + `ThreadedCompositor` + `PropertyTrees::build()` + `scroll_page_by`. 15 тестов. |
| 2026-06-15 | PH1-6: Stacking contexts + CSS Painting Order | `build_display_list_ordered` подключён к driver; 3 теста на CSS 2.1 Appendix E. |
| 2026-06-15 | PH1-5: CI/CD для Linux/macOS/Windows | `.github/workflows/ci.yml` + `release.yml`; 4 бинарных пакета. |
| 2026-06-15 | PH1-4: Network service в отдельном процессе | `lumen-ipc` крейт; `RemoteNetworkTransport`; `--network-service` флаг. |
| 2026-06-15 | PH1-15: T1 (paused) | `pause_event_loop()`/`unpause_event_loop()` в `PersistentJs`; 6 тестов. |
| 2026-06-15 | PH1-2: Progressive / streaming rendering pipeline | 60 Hz throttle; `LoadEvent::CssLoaded`; параллельная загрузка CSS; 3 теста. |
| 2026-06-15 | PH1-9: lumen-mcp-server крейт | 5 ресурсов + 7 инструментов; StdioTransport + TcpTransport; shell `--mcp` / `--mcp-port N`. 15 тестов. |
| 2026-06-14 | PH1-10..14: Auto-wait / Per-context isolation / A11y first-class / TabState / Image LRU | Все подтверждены в коде; STATUS обновлён. |
| 2026-06-14 | PH2-1/4/5/6/8/9/10/11/12/15/16: Phase 2 features | HTTP/2, anti-fingerprinting, meta viewport, Shadow DOM runtime, IME, mix-blend-mode stacking, BiDi, GPU LRU, Glyph LRU — все подтверждены. |
| 2026-06-14 | Y-series (Y-2..Y-5): Web Platform Phase 4 | unicode-range в lumen-font, scrollbar-width/color, color-scheme, scroll snap events — все реализованы. |
