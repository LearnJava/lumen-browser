# Perf-аудит Lumen vs Edge на lenta.ru — 2026-07-07

Полная запись исследования CPU/памяти, чтобы не повторять его заново.
Итог: **BUG-271 (CPU) FIXED**, **BUG-272 (память) — корень найден, срез 1 влит**.
Статусы и остаток — в [BUGS.md](../BUGS.md), [bugs/BUG-271-FIXED.md](../bugs/BUG-271-FIXED.md), [bugs/BUG-272-OPEN.md](../bugs/BUG-272-OPEN.md).

---

## Исходная жалоба

lumen.exe на lenta.ru: **2 417 МБ RAM, ~40% CPU** в диспетчере задач (Brave на той же машине ~450 МБ).

## Методика замеров (переиспользуемая)

| Инструмент | Команда / место | Что даёт |
|---|---|---|
| Память процесса | `powershell Get-Process lumen` → `WorkingSet64` / `PrivateMemorySize64` | WS/private в динамике |
| CPU в простое | два сэмпла `$p.CPU` с интервалом | CPU-сек/с после загрузки |
| Пер-поточный CPU | `$p.Threads → TotalProcessorTime` + `GetThreadDescription` (kernel32) | какой поток горит (`main`, `lumen-js`) |
| **GPU-память процесса** | `Get-Counter '\GPU Process Memory(pid_<PID>*)\Local Usage'` | ключевой инструмент BUG-272: на интегрированной графике это системная RAM, видимая как private процесса |
| Дамп хранилищ | `LUMEN_MEM_REPORT=1 lumen.exe <url>` (осталось в коде, `about_to_wait`) | размеры dl-cache / image / prefetch / webfonts / GIF / QuickJS heap / femtovg raw_images + layer_pool |
| Живая Rust-куча | временный counting `#[global_allocator]` (код удалён; см. историю ветки `p3-bug-272`) | «живые» аллокации vs фрагментация |
| Изоляция пути | headless `--screenshot` (JS+layout+CPU-paint, без окна) vs окно; `file://`-копия страницы | отсекает окно/femtovg и streaming соответственно |
| Изоляция источника redraw | временные счётчики по источникам `request_redraw` (код удалён; история ветки `p3-bug-271`) | кто крутит кадры |
| Эталон | Edge с изолированным профилем `--user-data-dir`, сумма процессов по `CommandLine like '%profile%'` | честное сравнение |

## Замеры «до» (2026-07-07, release)

| Метрика | Lumen | Edge (полная страница, реклама) |
|---|---|---|
| WS | 1 371 МБ (у пользователя 2 417 — вероятно, несколько навигаций) | ~530 МБ на 8–9 процессов |
| Private | 1 124 МБ | ~290 МБ |
| CPU после загрузки | ~1.6 ядра постоянно, старт через ~25–30 с | ~0.3 CPU-с/с |
| Контент | 1 картинка (easylist порезал трекеры), 1913 DOM-узлов, 933 CSS-правила, 1874 paint-команд, 7 webfonts | всё |

Baseline `samples/page.html`: 328 МБ WS / 138 МБ private, CPU в простое 0.

## BUG-271 (CPU) — путь к корню

Проверенные и **опровергнутые** гипотезы:
1. «JS-таймеры гоняют полный re-render» — setInterval-страницы дают ~0 CPU.
2. «rAF-цикл гонит 60 fps repaint» — частично: чистый rAF-цикл жёг 0.95 ядра (починено rAF-насосом), но lenta жгла и после этого фикса.
3. Счётчики показали: **0 кадров рисуется**, но `about_to_wait` крутится **~22 800 итераций/с**; горят потоки `main` (0.7 ядра) и `lumen-js` (0.94).

**Корень:** протухший `ControlFlow::WaitUntil` — после отработки последнего JS-таймера (`take_timer_wakeup()` → `None`) дедлайн в прошлом оставался в winit = Poll-подобный spin; каждая итерация делала 4+ блокирующих eval-round-trip в поток `lumen-js` (`tick_timers`, `pump_websockets`, `pump_sse`, `flush_canvas_updates` — все безусловные). Отложенный старт burn'а = момент догорания последней таймер-цепочки.

**Фикс (влит, 6dcf20ee):** явный `Wait`/`WaitUntil(min(таймер, rAF))` каждый проход; rAF-насос в `about_to_wait` (repaint только при `take_dom_dirty`); кламп вложенных таймеров ≥4 мс (HTML §8.6). lenta: 22 800 → 2 пробуждения/с, CPU ~0. Инвариант задокументирован в [subsystems/shell.md](../subsystems/shell.md).

## BUG-272 (память) — путь к корню

Проверенные и **опровергнутые** гипотезы (не проверять заново):
1. **Многокопийные image-кэши** (femtovg `raw_images` deep-copy, `@WxH`-текстуры, шелл-кэш, decode-кэш 256 МБ, GIF все кадры, font `bytes_store` без лимита + `.cloned()`, canvas2d thread-local без очистки) — статический аудит подтвердил их существование, но на lenta загружена 1 картинка → НЕ главный потребитель здесь. Актуально для image-heavy сайтов — отложено (список в файле бага).
2. **Известные хранилища** — MEM_REPORT: суммарно ~17 МБ.
3. **QuickJS heap** — 13 МБ.
4. **Rust-куча целиком** — counting-allocator: живых 119 МБ, пик 144 МБ → гигабайт живёт ВНЕ Rust-аллокатора.
5. **Streaming-кадры** — `file://`-копия (мгновенный HTML) даёт ту же память.
6. **JS/DOM/layout/CPU-paint** — headless `--screenshot` той же страницы: пик 94 МБ.
7. TileGrid (только флаги), `promote_will_change_layers` (no-op в femtovg), PushScrollLayer (scissor) — без текстур.

**Корень:** GPU-память процесса 1 168 МБ. Каждый `Push{ClipRoundedRect,ClipPath,Opacity,Filter,Mask,Backdrop}` в femtovg создавал offscreen-текстуру **размером со весь framebuffer** (~5 МБ) и освобождал её через `*_pending_delete` только **после `canvas.flush()`** в конце кадра → ~150 слоёв за кадр живы одновременно (~750 МБ), драйвер удерживает пик навсегда. Подтверждено синтетикой: 120 блоков 50×20 с `border-radius+overflow:hidden` = 1 025 МБ GPU.

**Фикс, срез 1 (влит, 0d9df0ea):** `FemtovgBackend::layer_pool` — release на Pop → reuse на следующем Push (femtovg исполняет очередь строго по порядку, перезапись пикселей безопасна, delete — нет). Пик = глубина вложенности (lenta: 3), кап 8. Инвариант — в [subsystems/paint.md](../subsystems/paint.md).

## Замеры «после»

| Метрика | До | После обоих фиксов |
|---|---|---|
| lenta.ru WS / private | 1 371 / 1 124 МБ | **713 / 497 МБ** |
| lenta.ru GPU | 1 168 МБ | 509 МБ |
| CPU в простое | ~1.6 ядра | ~0 (0.02 CPU-с/10с) |
| Синтетика 120 клипов GPU | 1 025 МБ | 227 МБ (= пустое окно) |

Корректность: полные графические прогоны (--ipc и оконный gdigrab) — дельта vs main нулевая.

## Что осталось (не потеряно, план в bugs/BUG-272-OPEN.md)

1. GPU на lenta 509 vs baseline 224 МБ (~285 МБ страничных): femtovg glyph atlas? blend-слои (PREMULTIPLIED) вне пула.
2. Baseline пустого окна 224 МБ GPU — сам по себе жирный.
3. Слои по bounding box вместо full-frame.
4. Многокопийные image-кэши для image-heavy сайтов (п.1 списка гипотез).
