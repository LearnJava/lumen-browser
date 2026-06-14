## 13. Безопасность

### 10.1 Sandboxing

- **Linux:** seccomp-bpf фильтр (whitelist syscalls), user namespaces, дополнительно Landlock для FS.
- **macOS:** App Sandbox через `sandbox_init`, entitlements в plist.
- **Windows:** AppContainer + Job Object + Restricted Token + Mitigation Policies (DEP, ASLR, CFG).

Каждый renderer-процесс — в своём сэндбоксе, без доступа к сети (только через IPC к network service) и без доступа к диску (только через IPC к storage service).

### 10.2 Memory safety

- Rust исключает 70% типичных CVE (use-after-free, buffer overflow, data races).
- `unsafe` — только в:
  - FFI к JS-движку (V8/QuickJS) — `engine/js-binding`,
  - FFI к декодерам, если используем C-либы (AVIF),
  - кастомных аренах DOM (когда индексы выходят за рамки borrow checker).
- Все `unsafe`-блоки помечены, документированы, ревью обязательно.
- `cargo-geiger` для мониторинга `unsafe` в зависимостях.

### 10.3 Process isolation

- Site isolation по eTLD+1.
- COOP / COEP / CORP — поддерживаем.
- `SharedArrayBuffer` — только с правильными заголовками (защита от Spectre).
- Process per origin для opaque origins (`data:`, sandboxed iframes).

### 10.4 Updates

- Подписанные релизы (minisign или sigstore).
- Update-проверка раз в день (можно отключить), не загружает ничего без согласия (или авто-загрузка, как опция).
- Roadmap — детерминированные сборки (reproducible builds) к 1.0.

### 10.5 Дополнительно

- CSP, Mixed Content, Subresource Integrity — строгие дефолты.
- HSTS preload list — встроенный, обновляемый.
- Certificate transparency — проверяем SCT.
- Safe Browsing — **НЕ используем Google API**. Опционально подключаем собственный список через DNS (например, Quad9 уже блокирует malware).
- Fuzzing: `cargo-fuzz` на HTML parser, CSS parser, image decoders, URL parser, JS-binding границы. Запуск в CI.

---

## 14. Производительность

### 11.1 Цели

| Метрика | Цель v0.1 | Цель v1.0 |
|---|---|---|
| Cold start до окна | < 300 мс | < 500 мс |
| Cold start до загруженной google.com | n/a | < 1.5 с |
| RAM на пустую вкладку | < 50 МБ | < 80 МБ |
| RAM на 5 типичных вкладок | < 250 МБ | < 600 МБ |
| RAM на 100 hibernated вкладок | < 200 МБ | < 300 МБ |
| Speedometer 3.0 | n/a | в пределах 2× от Chromium |
| Идл CPU (видимое окно) | < 1% | < 1% |

### 11.2 Стратегии

- **Параллельный layout / style** через `rayon` — главный архитектурный плюс перед Blink (Blink в этом плане монолитен).
- **Lazy tabs** — при восстановлении сессии вкладки не загружаются.
- **Tab hibernation** — освобождение renderer-процесса с сохранением навигации.
- **GPU-композитинг** — всё на wgpu.
- **Кэширование** — display list, computed styles переиспользуются при инвалидации.
- **Инвалидация** — точечная, не «пересчитать всё дерево».
- **Image decoding** — на отдельных тредах, прогрессивный.

### 11.3 Профилирование

- `tracy` интегрирован, активируется флагом `--profile`.
- Бенчмарки в CI: layout простой страницы, парсинг HTML 10 МБ, JS Speedometer.
- Tracking регрессий — графики по коммитам.

### 11.4 Memory budget per tab — пятитайерная модель ([ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md))

Главный продуктовый дифференциатор Lumen наряду с приватностью — **RAM-нагрузка на вкладку**. Цель: 50 открытых вкладок в Lumen занимают ~400 MB, в Chrome — 6-10 GB. Достигается за счёт явной модели жизненного цикла вкладки с пятью tier'ами и тремя структурными инвариантами на подсистемы.

#### Tier'ы T0–T4 и переходы

| Tier | Когда | Что в RAM | Бюджет (v0.1) |
|---|---|---|---|
| **T0 Active** | foreground, видимая | JS heap, DOM, layout, paint, image cache, GPU textures | 80-200 MB |
| **T1 Background-recent** | скрыта < 5 мин | JS heap paused, остальное retained | 40 MB |
| **T2 Background-old** | скрыта 5-30 мин | JS heap → snapshot на диск, image/GPU cache drop, layout retained | 15 MB |
| **T3 Hibernated** | скрыта >30 мин или memory pressure | DOM → сериализован в SQLite; в RAM только TabMetadata (URL, title, scroll, favicon) | 200 KB |
| **T4 Closed-recoverable** | закрыта пользователем | 0 RAM (entry в session history) | 0 |

Переходы между tier'ами — **OR трёх условий**: idle timeout + OS memory pressure + LRU within global budget. Pinned вкладки не уходят за T1 (явный пользовательский opt-in).

#### Restore SLO (binding)

| Переход | Цель |
|---|---|
| T1 → T0 | ≤ 50 ms (resume JS event loop) |
| T2 → T0 | ≤ 200 ms (restore JS heap + re-decode visible images) |
| T3 → T0 | ≤ 1500 ms (deserialize DOM, re-run scripts, full layout+paint) |
| T4 → T0 | network-bound (fresh navigation) |

Регрессия > 20% на любом переходе — release-blocker (см. `lumen-bench` RAM-axis, задача 9G.3).

#### Три структурных инварианта (binding на subsystems)

Эти инварианты **должны быть приняты до Phase 1 finalize** соответствующих крейтов, иначе ретрофит обойдётся в 5-10× по часам (см. ADR-008 «Context»).

1. **DOM = arena с `NodeId(u32)`, не `Rc<RefCell<Node>>` граф.** Сериализуется через `bincode` для T3. `lumen-dom` уже движется в эту сторону — ADR делает это формально-обязательным.
2. **JsRuntime поддерживает `suspend()` / `resume()` / `pause()` / `unpause()`** через `lumen-core::ext::JsRuntime` trait. QuickJS это умеет, V8 — нет out-of-the-box. **Закрепляет QuickJS как обязательный Phase 0-2 выбор**; миграция на V8 в Phase 3 (ADR-004) допустима только при доказанной возможности suspend через V8 snapshot API.
3. **Layout и paint — pure functions от `(DOM, stylesheet, viewport)`.** Никаких `static MUT`, никаких lazy_static / OnceCell в `lumen-layout` / `lumen-paint`. T2→T0 = просто пере-вызов функции. Исключение — cross-tab кэши (glyph atlas, font metrics, image decode) живут в своих крейтах с явным eviction API.

#### Техники экономии на активной вкладке (T0)

Не отложены на hibernation — работают **постоянно** для уменьшения T0:

- **Image decode cache LRU + viewport-gating.** Декодировать только то, что в viewport ± buffer. При скролле — decode/discard. `1920×1080 RGBA = 8 MB`; страница с 30 картинками без gating = 240 MB только на изображениях.
- **GPU layer LRU + texture recycling.** Off-viewport stacking contexts освобождают свои textures когда удалены от viewport больше N экранов.
- **Glyph atlas LRU eviction.** Атлас не растёт безгранично; редко используемые глифы вытесняются.
- **JS heap GC tuning.** QuickJS GC thresholds настраиваются per-tab; pinned tabs получают более мягкий GC, идлящие — более агрессивный.
- **`MemoryPressureSource` trait** (`lumen-core::ext`) ✅ — слушает OS-сигналы (Win32 `GlobalMemoryStatusEx`, Linux PSI `/proc/pressure/memory`, macOS `host_statistics64(HOST_VM_INFO64)`) и эмитит `Low / Medium / High` события. Подсистемы (caches, GPU layers, decoders) подписываются.

#### Сводные RAM-targets

Расширение §14.1 (binding numbers vs `bench/baseline.json`):

| Сценарий | Soft v0.1 | Hard v0.1 | Soft v1.0 | Hard v1.0 |
|---|---|---|---|---|
| T0 simple page (samples/page.html) | 80 MB | 100 MB | 150 MB | 200 MB |
| T0 heavy page (samples/heavy.html, Habr-style) | 150 MB | 200 MB | 250 MB | 350 MB |
| T1 per tab | 40 MB | 60 MB | 60 MB | 100 MB |
| T2 per tab | 15 MB | 25 MB | 25 MB | 40 MB |
| T3 per tab | 200 KB | 1 MB | 200 KB | 2 MB |
| 50 вкладок (1 active, остальные mixed T1/T2/T3) | 400 MB | 600 MB | 800 MB | 1200 MB |

---

